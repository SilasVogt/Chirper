import Adw from 'gi://Adw';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Gtk from 'gi://Gtk';

Gio._promisify(
    Gio.Subprocess.prototype,
    'communicate_utf8_async',
    'communicate_utf8_finish'
);
Gio._promisify(
    Gio.Subprocess.prototype,
    'wait_async',
    'wait_finish'
);

const PREFERRED_OLLAMA_MODELS = new Set(['granite4.1:8b', 'gemma3:4b']);
const MODE_OPTIONS = [
    ['auto', 'Auto'],
    ['standard', 'Standard'],
    ['email', 'Email'],
    ['command', 'Command'],
    ['code', 'Code'],
];
const PROMPT_INPUT_OPTIONS = [
    ['both', 'Raw + Preprocessed'],
    ['raw', 'Raw Only'],
    ['none', 'No Preprocessor'],
];
const CODEX_EFFORT_OPTIONS = [
    ['', 'Default Effort'],
    ['low', 'Low'],
    ['medium', 'Medium'],
    ['high', 'High'],
    ['xhigh', 'Extra High'],
];

let window = null;

async function runCommand(argv) {
    const process = Gio.Subprocess.new(
        argv,
        Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE
    );
    const [stdout, stderr] = await process.communicate_utf8_async(null, null);

    if (!process.get_successful())
        throw new Error((stderr ?? '').trim() || `${argv[0]} failed`);

    return stdout;
}

function readStreamLines(stream, onLine = null) {
    const reader = new Gio.DataInputStream({ base_stream: stream });
    const lines = [];

    return new Promise((resolve, reject) => {
        const readNext = () => {
            reader.read_line_async(GLib.PRIORITY_DEFAULT, null, (source, result) => {
                try {
                    const [line] = source.read_line_finish_utf8(result);
                    if (line === null) {
                        resolve(lines.join('\n'));
                        return;
                    }

                    lines.push(line);
                    if (onLine)
                        onLine(line);
                    readNext();
                } catch (error) {
                    reject(error);
                }
            });
        };

        readNext();
    });
}

async function runCommandStreaming(argv, onProgressLine) {
    const process = Gio.Subprocess.new(
        argv,
        Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE
    );
    const stdout = readStreamLines(process.get_stdout_pipe());
    const stderr = readStreamLines(process.get_stderr_pipe(), onProgressLine);

    await process.wait_async(null);
    const [stdoutText, stderrText] = await Promise.all([stdout, stderr]);

    if (!process.get_successful())
        throw new Error(stderrText.trim() || `${argv[0]} failed`);

    return stdoutText;
}

function cliPath() {
    return GLib.getenv('CHIRPER_CLI') || 'chirper';
}

function defaultReportDir() {
    return GLib.build_filenamev([
        GLib.get_home_dir(),
        'Documents',
        'Chirper Compare Reports',
    ]);
}

function makeStringList(options) {
    const list = new Gtk.StringList();
    for (const [, label] of options)
        list.append(label);
    return list;
}

function selectedOption(options, row) {
    return options[row.selected]?.[0] ?? options[0][0];
}

function entryText(row) {
    return (row.get_text ? row.get_text() : row.text).trim();
}

function textFromBuffer(buffer) {
    const [start, end] = buffer.get_bounds();
    return buffer.get_text(start, end, false);
}

function formatDurationMs(elapsedMs) {
    if (!Number.isFinite(elapsedMs) || elapsedMs < 0)
        return '0ms';

    if (elapsedMs < 1000)
        return `${Math.round(elapsedMs)}ms`;

    let seconds = Math.floor(elapsedMs / 1000);
    const hours = Math.floor(seconds / 3600);
    seconds %= 3600;
    const minutes = Math.floor(seconds / 60);
    seconds %= 60;

    if (hours > 0)
        return `${hours}h ${minutes}m ${seconds}s`;
    if (minutes > 0)
        return `${minutes}m ${seconds}s`;
    return `${seconds}s`;
}

function formatBytes(bytes) {
    if (!Number.isFinite(bytes))
        return null;

    const gib = 1024 ** 3;
    const mib = 1024 ** 2;
    if (bytes >= gib)
        return `${(bytes / gib).toFixed(2)} GiB`;
    if (bytes >= mib)
        return `${(bytes / mib).toFixed(1)} MiB`;
    return `${Math.round(bytes)} B`;
}

function shortGpuName(name) {
    if (!name)
        return null;

    const match = name.match(/\[(AMD\/ATI|NVIDIA|Intel)[^\]]*\]\s*(.*?)(?:\s*\(rev|$)/);
    if (match?.[2])
        return match[2].trim();

    return name;
}

function hardwareSummary(hardware) {
    if (!hardware)
        return 'Hardware unavailable';

    const parts = [];
    if (hardware.cpu_model)
        parts.push(hardware.cpu_model);
    if (hardware.gpu) {
        parts.push(shortGpuName(hardware.gpu.name) || hardware.gpu.card || 'GPU detected');
        const vram = formatBytes(hardware.gpu.vram_total_bytes);
        if (vram)
            parts.push(`${vram} VRAM`);
        if (Number.isFinite(hardware.gpu.power_watts))
            parts.push(`${Math.round(hardware.gpu.power_watts)} W now`);
    }
    const ram = formatBytes(hardware.ram_total_bytes);
    if (ram)
        parts.push(`${ram} RAM`);

    return parts.join(' | ') || 'Hardware unavailable';
}

function makeButton(label, callback, cssClass = null) {
    const control = new Gtk.Button({
        label,
        valign: Gtk.Align.CENTER,
    });

    if (cssClass)
        control.add_css_class(cssClass);

    control.connect('clicked', callback);
    return control;
}

function makeCheckRow(title, subtitle, active = false) {
    const row = new Adw.ActionRow({ title, subtitle });
    const check = new Gtk.CheckButton({
        active,
        valign: Gtk.Align.CENTER,
    });

    row.add_prefix(check);
    row.activatable_widget = check;
    return { row, check };
}

const ModelCompareWindow = GObject.registerClass(class ModelCompareWindow extends Adw.ApplicationWindow {
    constructor(application) {
        super({
            application,
            title: 'Chirper Model Compare',
            default_width: 980,
            default_height: 820,
        });

        this._ollamaRows = new Map();
        this._codexRows = new Map();
        this._dynamicRows = [];
        this._runButton = null;
        this._refreshButton = null;
        this._addCodexButton = null;
        this._elapsedTimerId = 0;
        this._runStartedUsec = 0;
        this._progressTotal = 0;
        this._outputText = '';
        this._build();
        this._refreshModels();
    }

    _build() {
        const toolbarView = new Adw.ToolbarView();
        const header = new Adw.HeaderBar({
            title_widget: new Adw.WindowTitle({
                title: 'Model Compare',
                subtitle: 'Compare Ollama and Codex formatter outputs',
            }),
        });
        toolbarView.add_top_bar(header);

        const scroller = new Gtk.ScrolledWindow({
            hscrollbar_policy: Gtk.PolicyType.NEVER,
            vexpand: true,
        });
        const clamp = new Adw.Clamp({
            maximum_size: 920,
            tightening_threshold: 720,
            margin_top: 18,
            margin_bottom: 18,
            margin_start: 18,
            margin_end: 18,
        });
        const main = new Gtk.Box({
            orientation: Gtk.Orientation.VERTICAL,
            spacing: 16,
        });

        clamp.set_child(main);
        scroller.set_child(clamp);
        toolbarView.set_content(scroller);
        this.set_content(toolbarView);

        this._buildTranscriptGroup(main);
        this._buildOptionsGroup(main);
        this._buildOllamaGroup(main);
        this._buildCodexGroup(main);
        this._buildProgressGroup(main);
        this._buildOutputGroup(main);
    }

    _buildTranscriptGroup(main) {
        const group = new Adw.PreferencesGroup({
            title: 'Transcript',
            description: 'Paste a raw Whisper transcript or a written test case.',
        });
        main.append(group);

        this._transcriptBuffer = new Gtk.TextBuffer();
        this._transcriptView = new Gtk.TextView({
            buffer: this._transcriptBuffer,
            monospace: true,
            wrap_mode: Gtk.WrapMode.WORD_CHAR,
            top_margin: 12,
            bottom_margin: 12,
            left_margin: 12,
            right_margin: 12,
        });
        this._transcriptView.add_css_class('view');

        const frame = new Gtk.ScrolledWindow({
            min_content_height: 150,
            max_content_height: 230,
            hscrollbar_policy: Gtk.PolicyType.NEVER,
            child: this._transcriptView,
        });
        frame.add_css_class('card');
        group.add(frame);
    }

    _buildOptionsGroup(main) {
        const group = new Adw.PreferencesGroup({ title: 'Run Options' });
        main.append(group);

        this._modeRow = new Adw.ComboRow({
            title: 'Dictation Mode',
            model: makeStringList(MODE_OPTIONS),
            selected: 0,
        });
        group.add(this._modeRow);

        this._promptInputRow = new Adw.ComboRow({
            title: 'Prompt Input',
            subtitle: 'Choose whether models see rules output or only the raw transcript.',
            model: makeStringList(PROMPT_INPUT_OPTIONS),
            selected: 0,
        });
        group.add(this._promptInputRow);

        this._includeRulesRow = new Adw.SwitchRow({
            title: 'Show Rules Baseline',
            subtitle: 'Include deterministic preprocessor output in the result.',
            active: true,
        });
        group.add(this._includeRulesRow);

        this._keepLoadedRow = new Adw.SwitchRow({
            title: 'Keep Ollama Models Loaded',
            subtitle: 'Useful for warm repeated runs, but can keep VRAM occupied.',
            active: false,
        });
        group.add(this._keepLoadedRow);

        this._writeReportRow = new Adw.SwitchRow({
            title: 'Write Report File',
            subtitle: 'Stores outputs, timings, hardware, and telemetry samples.',
            active: true,
        });
        group.add(this._writeReportRow);

        this._reportDirRow = new Adw.EntryRow({
            title: 'Report Folder',
            text: defaultReportDir(),
        });
        group.add(this._reportDirRow);

        this._promptNoteBuffer = new Gtk.TextBuffer();
        const promptNoteView = new Gtk.TextView({
            buffer: this._promptNoteBuffer,
            wrap_mode: Gtk.WrapMode.WORD_CHAR,
            top_margin: 10,
            bottom_margin: 10,
            left_margin: 12,
            right_margin: 12,
        });
        promptNoteView.add_css_class('view');
        const promptFrame = new Gtk.ScrolledWindow({
            min_content_height: 86,
            max_content_height: 140,
            hscrollbar_policy: Gtk.PolicyType.NEVER,
            child: promptNoteView,
        });
        promptFrame.add_css_class('card');
        const promptRow = new Adw.ActionRow({
            title: 'Extra Prompt Instructions',
            subtitle: 'Optional. Applied only to this comparison run.',
        });
        group.add(promptRow);
        group.add(promptFrame);

        const actionRow = new Adw.ActionRow({
            title: 'Compare',
            subtitle: 'Runs selected targets sequentially.',
        });
        this._refreshButton = makeButton('Refresh Models', () => this._refreshModels());
        this._runButton = makeButton('Run Compare', () => this._runCompare(), 'suggested-action');
        actionRow.add_suffix(this._refreshButton);
        actionRow.add_suffix(this._runButton);
        actionRow.activatable_widget = this._runButton;
        group.add(actionRow);
    }

    _buildOllamaGroup(main) {
        this._ollamaGroup = new Adw.PreferencesGroup({
            title: 'Ollama Models',
            description: 'Tick the installed models to include in the next compare run.',
        });
        main.append(this._ollamaGroup);

        this._allOllamaRow = new Adw.SwitchRow({
            title: 'Run All Installed Ollama Models',
            subtitle: 'Usually slow. Leave off to run only ticked models.',
            active: false,
        });
        this._ollamaGroup.add(this._allOllamaRow);
    }

    _buildCodexGroup(main) {
        this._codexGroup = new Adw.PreferencesGroup({
            title: 'Codex CLI',
            description: 'Tick saved Codex configurations or add one for future comparisons.',
        });
        main.append(this._codexGroup);

        this._codexCurrentRow = new Adw.SwitchRow({
            title: 'Run Current Codex Settings',
            subtitle: 'Uses `chirper codex-current` settings.',
            active: false,
        });
        this._codexGroup.add(this._codexCurrentRow);

        this._codexNameRow = new Adw.EntryRow({ title: 'New Config Name' });
        this._codexGroup.add(this._codexNameRow);

        this._codexModelRow = new Adw.EntryRow({
            title: 'Codex Model',
            text: 'gpt-5.5',
        });
        this._codexGroup.add(this._codexModelRow);

        this._codexProfileRow = new Adw.EntryRow({
            title: 'Codex CLI Profile',
        });
        this._codexGroup.add(this._codexProfileRow);

        this._codexEffortRow = new Adw.ComboRow({
            title: 'Reasoning Effort',
            model: makeStringList(CODEX_EFFORT_OPTIONS),
            selected: 0,
        });
        this._codexGroup.add(this._codexEffortRow);

        this._codexFastRow = new Adw.SwitchRow({
            title: 'Fast / Priority Tier',
            subtitle: 'Stores Codex service_tier=priority for this config.',
            active: false,
        });
        this._codexGroup.add(this._codexFastRow);

        this._codexConfigRow = new Adw.EntryRow({
            title: 'Extra Codex -c Override',
        });
        this._codexGroup.add(this._codexConfigRow);

        const addRow = new Adw.ActionRow({
            title: 'Save Codex Config',
            subtitle: 'The saved config appears as a tickable target below.',
        });
        this._addCodexButton = makeButton('Add Config', () => this._addCodexProfile());
        addRow.add_suffix(this._addCodexButton);
        addRow.activatable_widget = this._addCodexButton;
        this._codexGroup.add(addRow);
    }

    _buildProgressGroup(main) {
        this._progressGroup = new Adw.PreferencesGroup({
            title: 'Run Progress',
            description: 'Visible while a comparison is running.',
        });
        this._progressGroup.visible = false;
        main.append(this._progressGroup);

        this._progressBar = new Gtk.ProgressBar({
            show_text: true,
            margin_top: 8,
            margin_bottom: 8,
            margin_start: 12,
            margin_end: 12,
        });
        this._progressBar.set_fraction(0);
        this._progressBar.set_text('Idle');
        this._progressGroup.add(this._progressBar);

        this._currentTargetRow = new Adw.ActionRow({
            title: 'Current Model',
            subtitle: 'Idle',
        });
        this._progressGroup.add(this._currentTargetRow);

        this._elapsedRow = new Adw.ActionRow({
            title: 'Runtime',
            subtitle: '0ms',
        });
        this._progressGroup.add(this._elapsedRow);

        this._hardwareRow = new Adw.ActionRow({
            title: 'Hardware',
            subtitle: 'Waiting for run',
        });
        this._progressGroup.add(this._hardwareRow);

        this._summaryRow = new Adw.ActionRow({
            title: 'Summary',
            subtitle: 'Not run yet',
        });
        this._progressGroup.add(this._summaryRow);
    }

    _buildOutputGroup(main) {
        const group = new Adw.PreferencesGroup({ title: 'Output' });
        main.append(group);

        this._statusRow = new Adw.ActionRow({
            title: 'Status',
            subtitle: 'Ready',
        });
        group.add(this._statusRow);

        this._outputBuffer = new Gtk.TextBuffer();
        const outputView = new Gtk.TextView({
            buffer: this._outputBuffer,
            editable: false,
            cursor_visible: false,
            monospace: true,
            wrap_mode: Gtk.WrapMode.WORD_CHAR,
            top_margin: 12,
            bottom_margin: 12,
            left_margin: 12,
            right_margin: 12,
        });
        outputView.add_css_class('view');

        const frame = new Gtk.ScrolledWindow({
            min_content_height: 260,
            hscrollbar_policy: Gtk.PolicyType.NEVER,
            vexpand: true,
            child: outputView,
        });
        frame.add_css_class('card');
        group.add(frame);
    }

    async _refreshModels() {
        this._setBusy(true, 'Refreshing model lists');
        this._clearDynamicRows();

        await Promise.all([
            this._refreshOllamaModels(),
            this._refreshCodexProfiles(),
        ]);

        this._setBusy(false, 'Ready');
    }

    _clearDynamicRows() {
        for (const row of this._dynamicRows)
            row.get_parent()?.remove(row);

        this._dynamicRows = [];
        this._ollamaRows.clear();
        this._codexRows.clear();
    }

    async _refreshOllamaModels() {
        try {
            const output = await runCommand([cliPath(), 'ollama-list', '--json']);
            const data = JSON.parse(output);
            const current = data.current?.model;
            const models = data.models ?? [];

            if (!data.available) {
                this._addInfoRow(this._ollamaGroup, data.error ?? 'Ollama unavailable');
                return;
            }

            if (models.length === 0) {
                this._addInfoRow(this._ollamaGroup, 'No Ollama models found');
                return;
            }

            const preferredInstalled = models.some(candidate =>
                PREFERRED_OLLAMA_MODELS.has(candidate.name)
            );
            for (const model of models) {
                const selected = PREFERRED_OLLAMA_MODELS.has(model.name) ||
                    (!preferredInstalled && model.name === current);
                const { row, check } = makeCheckRow(
                    model.name,
                    model.name === current ? 'Current formatter model' : 'Installed',
                    selected
                );
                this._ollamaRows.set(model.name, { row, check });
                this._ollamaGroup.add(row);
                this._dynamicRows.push(row);
            }
        } catch (error) {
            this._addInfoRow(this._ollamaGroup, error.message);
        }
    }

    async _refreshCodexProfiles() {
        try {
            const output = await runCommand([cliPath(), 'codex-current', '--json']);
            const data = JSON.parse(output);
            const current = data.current ?? {};
            const profiles = data.profiles ?? [];

            this._codexCurrentRow.subtitle = data.available
                ? `Current: ${current.label ?? 'codex-default'}`
                : 'Codex CLI unavailable, saved profiles can still be edited';

            if (current.model && !entryText(this._codexModelRow))
                this._codexModelRow.text = current.model;

            if (profiles.length === 0) {
                this._addInfoRow(this._codexGroup, 'No Codex configs saved yet');
                return;
            }

            for (const profile of profiles) {
                const detail = [
                    profile.model,
                    profile.reasoning_effort ? `effort=${profile.reasoning_effort}` : null,
                    profile.service_tier ? `tier=${profile.service_tier}` : null,
                    profile.profile ? `profile=${profile.profile}` : null,
                ].filter(Boolean).join(', ') || 'Codex defaults';
                const { row, check } = makeCheckRow(profile.name, detail, false);
                const removeButton = new Gtk.Button({
                    icon_name: 'user-trash-symbolic',
                    valign: Gtk.Align.CENTER,
                });
                removeButton.add_css_class('flat');
                removeButton.set_tooltip_text(`Remove ${profile.name}`);
                removeButton.connect('clicked', () => this._removeCodexProfile(profile.name));
                row.add_suffix(removeButton);

                this._codexRows.set(profile.name, { row, check });
                this._codexGroup.add(row);
                this._dynamicRows.push(row);
            }
        } catch (error) {
            this._codexCurrentRow.subtitle = error.message;
        }
    }

    _addInfoRow(group, message) {
        const row = new Adw.ActionRow({
            title: message,
            sensitive: false,
        });
        group.add(row);
        this._dynamicRows.push(row);
    }

    async _addCodexProfile() {
        const name = entryText(this._codexNameRow);
        if (!name) {
            this._setStatus('Enter a Codex config name first');
            return;
        }

        const args = ['codex-profile-add', name];
        const model = entryText(this._codexModelRow);
        const profile = entryText(this._codexProfileRow);
        const effort = selectedOption(CODEX_EFFORT_OPTIONS, this._codexEffortRow);
        const configOverride = entryText(this._codexConfigRow);

        if (model)
            args.push('--model', model);
        if (profile)
            args.push('--profile', profile);
        if (effort)
            args.push('--effort', effort);
        if (this._codexFastRow.active)
            args.push('--fast');
        if (configOverride)
            args.push('--config', configOverride);

        this._setBusy(true, `Saving Codex config ${name}`);
        try {
            const output = await runCommand([cliPath(), ...args]);
            this._outputBuffer.set_text(output, -1);
            this._codexNameRow.text = '';
            await this._refreshModels();
            this._setStatus(`Saved Codex config ${name}`);
        } catch (error) {
            this._outputBuffer.set_text(error.message, -1);
            this._setBusy(false, 'Could not save Codex config');
        }
    }

    async _removeCodexProfile(name) {
        this._setBusy(true, `Removing Codex config ${name}`);
        try {
            const output = await runCommand([cliPath(), 'codex-profile-remove', name]);
            this._outputBuffer.set_text(output, -1);
            await this._refreshModels();
            this._setStatus(`Removed Codex config ${name}`);
        } catch (error) {
            this._outputBuffer.set_text(error.message, -1);
            this._setBusy(false, 'Could not remove Codex config');
        }
    }

    async _runCompare() {
        const transcript = textFromBuffer(this._transcriptBuffer).trim();
        if (!transcript) {
            this._setStatus('Paste a transcript first');
            return;
        }

        const args = this._buildCompareArgs(transcript);
        this._resetRunProgress();
        this._setOutput(`$ ${[cliPath(), ...args].join(' ')}\n\n`);
        this._setBusy(true, 'Running comparison');
        this._startElapsedTimer();

        try {
            const output = await runCommandStreaming(
                [cliPath(), ...args],
                line => this._handleProgressLine(line)
            );
            this._stopElapsedTimer();
            if (output.trim())
                this._appendOutput(`\n${output}`);
            this._setBusy(false, 'Comparison finished');
        } catch (error) {
            this._stopElapsedTimer();
            this._appendOutput(`\n${error.message}\n`);
            this._setBusy(false, 'Comparison failed');
        }
    }

    _buildCompareArgs(transcript) {
        const args = ['format-compare', '--progress-json'];
        const mode = selectedOption(MODE_OPTIONS, this._modeRow);
        const promptInput = selectedOption(PROMPT_INPUT_OPTIONS, this._promptInputRow);
        const promptNote = textFromBuffer(this._promptNoteBuffer).trim();
        const selectedOllamaModels = [...this._ollamaRows.entries()]
            .filter(([, item]) => item.check.active)
            .map(([name]) => name);
        const selectedCodexProfiles = [...this._codexRows.entries()]
            .filter(([, item]) => item.check.active)
            .map(([name]) => name);

        args.push('--mode', mode);

        if (promptInput === 'raw')
            args.push('--prompt-input', 'raw');
        else if (promptInput === 'none')
            args.push('--no-preprocessor');
        else
            args.push('--prompt-input', 'both');

        if (!this._includeRulesRow.active && promptInput !== 'none')
            args.push('--no-rules');

        if (this._keepLoadedRow.active)
            args.push('--keep-loaded');

        if (this._allOllamaRow.active)
            args.push('--all-ollama');
        else if (selectedOllamaModels.length > 0)
            args.push('--models', selectedOllamaModels.join(','));
        else
            args.push('--no-ollama');

        if (this._codexCurrentRow.active)
            args.push('--codex');

        if (selectedCodexProfiles.length > 0)
            args.push('--codex-profile', selectedCodexProfiles.join(','));

        if (promptNote)
            args.push('--prompt-note', promptNote);

        const reportDir = entryText(this._reportDirRow);
        if (this._writeReportRow.active && reportDir)
            args.push('--report-dir', reportDir);

        args.push(transcript);
        return args;
    }

    _resetRunProgress() {
        this._progressGroup.visible = true;
        this._progressTotal = 0;
        this._progressBar.set_fraction(0);
        this._progressBar.set_text('Starting');
        this._currentTargetRow.title = 'Current Model';
        this._currentTargetRow.subtitle = 'Waiting for first model';
        this._elapsedRow.subtitle = '0ms';
        this._hardwareRow.subtitle = 'Collecting hardware snapshot';
        this._summaryRow.subtitle = 'Starting comparison';
    }

    _handleProgressLine(line) {
        let event = null;
        try {
            event = JSON.parse(line);
        } catch {
            if (line.trim())
                this._appendOutput(`${line}\n`);
            return;
        }

        switch (event.type) {
        case 'started':
            this._progressTotal = event.total ?? 0;
            this._hardwareRow.subtitle = hardwareSummary(event.hardware);
            this._summaryRow.subtitle = this._progressTotal > 0
                ? `Testing ${this._progressTotal} model targets`
                : 'Rules-only comparison';
            this._setProgress(0, this._progressTotal);
            break;
        case 'target_started':
            this._currentTargetRow.title = event.name ?? 'Current Model';
            this._currentTargetRow.subtitle = `Testing ${event.index ?? 0} of ${event.total ?? this._progressTotal}`;
            this._setProgress((event.index ?? 1) - 1, event.total ?? this._progressTotal);
            this._appendOutput(`Testing ${event.name}\n`);
            break;
        case 'target_finished': {
            const index = event.index ?? 0;
            const total = event.total ?? this._progressTotal;
            this._setProgress(index, total);
            this._currentTargetRow.title = event.name ?? 'Current Model';
            this._currentTargetRow.subtitle = event.ok
                ? `Finished in ${formatDurationMs(event.elapsed_ms ?? 0)}`
                : `Failed after ${formatDurationMs(event.elapsed_ms ?? 0)}`;
            this._summaryRow.subtitle = `Finished ${index} of ${total}`;
            const status = event.ok ? 'done' : `error: ${event.error ?? 'unknown error'}`;
            this._appendOutput(`${event.name}: ${status} (${formatDurationMs(event.elapsed_ms ?? 0)})\n`);
            break;
        }
        case 'finished': {
            this._setProgress(event.total ?? this._progressTotal, event.total ?? this._progressTotal);
            const summary = `Tested ${event.tested_models ?? 0} Models in ${formatDurationMs(event.elapsed_ms ?? 0)}`;
            this._summaryRow.subtitle = event.report_path
                ? `${summary} | ${event.report_path}`
                : summary;
            this._currentTargetRow.title = 'Complete';
            this._currentTargetRow.subtitle = summary;
            this._elapsedRow.subtitle = formatDurationMs(event.elapsed_ms ?? 0);
            this._appendOutput(`${summary}\n`);
            if (event.report_path)
                this._appendOutput(`Report: ${event.report_path}\n`);
            break;
        }
        default:
            break;
        }
    }

    _setProgress(done, total) {
        if (total > 0) {
            const fraction = Math.max(0, Math.min(1, done / total));
            this._progressBar.set_fraction(fraction);
            this._progressBar.set_text(`${done} / ${total}`);
        } else {
            this._progressBar.set_fraction(0);
            this._progressBar.set_text('No model targets');
        }
    }

    _startElapsedTimer() {
        this._stopElapsedTimer();
        this._runStartedUsec = GLib.get_monotonic_time();
        this._elapsedTimerId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 1, () => {
            this._updateElapsedRuntime();
            return true;
        });
    }

    _stopElapsedTimer() {
        if (this._elapsedTimerId) {
            GLib.source_remove(this._elapsedTimerId);
            this._elapsedTimerId = 0;
        }
        this._updateElapsedRuntime();
    }

    _updateElapsedRuntime() {
        if (!this._runStartedUsec)
            return;

        const elapsedMs = (GLib.get_monotonic_time() - this._runStartedUsec) / 1000;
        this._elapsedRow.subtitle = formatDurationMs(elapsedMs);
    }

    _setOutput(text) {
        this._outputText = text;
        this._outputBuffer.set_text(this._outputText, -1);
    }

    _appendOutput(text) {
        this._outputText += text;
        this._outputBuffer.set_text(this._outputText, -1);
    }

    _setBusy(busy, status) {
        if (this._runButton)
            this._runButton.sensitive = !busy;
        if (this._refreshButton)
            this._refreshButton.sensitive = !busy;
        if (this._addCodexButton)
            this._addCodexButton.sensitive = !busy;
        this._setStatus(status);
    }

    _setStatus(status) {
        this._statusRow.subtitle = status;
    }
});

const app = new Adw.Application({
    application_id: 'dev.local.Chirper.ModelCompare',
    flags: Gio.ApplicationFlags.FLAGS_NONE,
});

app.connect('activate', application => {
    window = new ModelCompareWindow(application);
    window.present();
});

app.run([GLib.get_prgname() ?? 'chirper-model-compare']);
