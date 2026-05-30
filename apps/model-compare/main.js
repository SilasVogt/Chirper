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
        this._outputBuffer.set_text(`$ ${[cliPath(), ...args].join(' ')}\n\n`, -1);
        this._setBusy(true, 'Running comparison');

        try {
            const output = await runCommand([cliPath(), ...args]);
            this._outputBuffer.set_text(output, -1);
            this._setBusy(false, 'Comparison finished');
        } catch (error) {
            this._outputBuffer.set_text(error.message, -1);
            this._setBusy(false, 'Comparison failed');
        }
    }

    _buildCompareArgs(transcript) {
        const args = ['format-compare'];
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
