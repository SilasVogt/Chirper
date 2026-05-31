import Adw from 'gi://Adw';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Gtk from 'gi://Gtk';
import Pango from 'gi://Pango';

Gio._promisify(
    Gio.Subprocess.prototype,
    'communicate_utf8_async',
    'communicate_utf8_finish'
);

const INPUT_OPTIONS = [
    ['previous', 'Previous Stage'],
    ['transcript', 'Original Transcript'],
];
const DEFAULT_PROMPTS = {
    fix: {
        name: 'Fix transcription errors',
        model: 'granite4.1:3b',
        inputMode: 'transcript',
        prompt: `You are a transcript cleanup stage.
Fix likely speech-to-text errors while preserving the speaker's words and intent.
Apply explicit spoken corrections, spelling instructions, punctuation, URLs, emails, identifiers, and casing clues.
Return only the corrected transcript. Do not explain.

Transcript:
{input}`,
    },
    search: {
        name: 'Find vocabulary lookups',
        model: 'granite4.1:3b',
        inputMode: 'previous',
        prompt: `You inspect a transcript for words or names that may need contextual vocabulary lookup.
Return only strict JSON. Do not include markdown.

Output schema:
{
  "action": "vocab_search",
  "queries": [
    {
      "spoken": "phrase to search",
      "surrounding_text": "short local context from the transcript",
      "reason": "why this may need a saved spelling"
    }
  ]
}

If no lookup is needed, return:
{"action":"vocab_search","queries":[]}

Transcript:
{input}`,
    },
    final: {
        name: 'Apply final formatting',
        model: 'rnj-1:8b',
        inputMode: 'previous',
        prompt: `You are the final dictation formatter.
Return only the final text that should be pasted at the user's cursor.
Use the current input as the corrected transcript.
Use prior stage outputs as advisory context, especially vocabulary lookup JSON.
Do not invent facts. Do not explain.

Original transcript:
{transcript}

Prior stage outputs:
{outputs}

Current input:
{input}`,
    },
    custom: {
        name: 'Custom stage',
        model: 'granite4.1:3b',
        inputMode: 'previous',
        prompt: `Return only the transformed text.

Input:
{input}`,
    },
};

let window = null;

function cliPath() {
    return GLib.getenv('CHIRPER_CLI') || 'chirper';
}

function defaultOutputDir() {
    return GLib.build_filenamev([
        GLib.get_home_dir(),
        'Documents',
        'Chirper Workflow Runs',
    ]);
}

function statePath() {
    return GLib.build_filenamev([
        GLib.get_user_config_dir(),
        'chirper',
        'workflow-builder.json',
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

function textFromBuffer(buffer) {
    const [start, end] = buffer.get_bounds();
    return buffer.get_text(start, end, false);
}

function setBufferText(buffer, text) {
    buffer.set_text(text ?? '', -1);
}

function entryText(row) {
    return (row.get_text ? row.get_text() : row.text).trim();
}

function readJsonFile(path) {
    try {
        const [ok, bytes] = GLib.file_get_contents(path);
        if (!ok)
            return null;

        return JSON.parse(new TextDecoder().decode(bytes));
    } catch {
        return null;
    }
}

function writeTextFile(path, text) {
    GLib.mkdir_with_parents(GLib.path_get_dirname(path), 0o755);
    GLib.file_set_contents(path, text ?? '');
}

function writeJsonFile(path, value) {
    writeTextFile(path, `${JSON.stringify(value, null, 2)}\n`);
}

function safeFilePart(value, fallback) {
    const slug = value
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, '-')
        .replace(/^-+|-+$/g, '');
    return slug || fallback;
}

function timestamp() {
    return GLib.DateTime
        .new_now_local()
        .format('%Y%m%d-%H%M%S');
}

function compactText(text, limit = 180) {
    const compact = text.replace(/\s+/g, ' ').trim();
    if (compact.length <= limit)
        return compact;

    return `${compact.slice(0, Math.max(0, limit - 1))}...`;
}

function makeButton(label, callback, cssClass = null) {
    const button = new Gtk.Button({
        label,
        valign: Gtk.Align.CENTER,
    });
    if (cssClass)
        button.add_css_class(cssClass);
    button.connect('clicked', callback);
    return button;
}

function makeIconButton(iconName, tooltip, callback) {
    const button = new Gtk.Button({
        icon_name: iconName,
        valign: Gtk.Align.CENTER,
    });
    button.add_css_class('flat');
    button.set_tooltip_text(tooltip);
    button.connect('clicked', callback);
    return button;
}

function makeTextView(buffer, editable = true, monospace = false) {
    const view = new Gtk.TextView({
        buffer,
        editable,
        cursor_visible: editable,
        monospace,
        wrap_mode: Gtk.WrapMode.WORD_CHAR,
        top_margin: 10,
        bottom_margin: 10,
        left_margin: 10,
        right_margin: 10,
    });
    view.add_css_class('view');
    return view;
}

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

function defaultWorkflow() {
    return {
        workflowName: 'dictation-workflow',
        outputDir: defaultOutputDir(),
        transcript: '',
        stages: [
            { ...DEFAULT_PROMPTS.fix },
            { ...DEFAULT_PROMPTS.search },
            { ...DEFAULT_PROMPTS.final },
        ],
    };
}

function renderPrompt(template, context) {
    let prompt = template.trim();
    const hadPlaceholder = /\{(?:input|transcript|previous|outputs|stage:[^}]+)\}/.test(prompt);

    prompt = prompt
        .replace(/\{input\}/g, context.input)
        .replace(/\{transcript\}/g, context.transcript)
        .replace(/\{previous\}/g, context.previous);

    prompt = prompt.replace(/\{stage:([^}]+)\}/g, (_match, name) => {
        return context.outputsByName.get(name.trim()) ?? '';
    });
    prompt = prompt.replace(/\{outputs\}/g, context.outputsText);

    if (!hadPlaceholder) {
        prompt += `\n\nInput:\n<<<\n${context.input}\n>>>`;
    }

    return prompt;
}

const WorkflowBuilderWindow = GObject.registerClass(class WorkflowBuilderWindow extends Adw.ApplicationWindow {
    constructor(application) {
        super({
            application,
            title: 'Chirper Test Workflow Builder',
            default_width: 1560,
            default_height: 980,
        });

        this._stageWidgets = new Map();
        this._stages = [];
        this._installedModels = [];
        this._currentProcess = null;
        this._currentRunDir = null;
        this._running = false;

        this._build();
        this._loadState();
        this._refreshModels();
    }

    _build() {
        const toolbarView = new Adw.ToolbarView();
        const header = new Adw.HeaderBar({
            title_widget: new Adw.WindowTitle({
                title: 'Test Workflow Builder',
                subtitle: 'Chain local model stages and inspect every output',
            }),
        });
        this._saveHeaderButton = new Gtk.Button({ icon_name: 'document-save-symbolic' });
        this._saveHeaderButton.set_tooltip_text('Save workflow');
        this._saveHeaderButton.connect('clicked', () => this._saveState());
        header.pack_end(this._saveHeaderButton);
        toolbarView.add_top_bar(header);

        const split = new Gtk.Paned({
            orientation: Gtk.Orientation.HORIZONTAL,
            shrink_start_child: false,
            shrink_end_child: false,
            resize_start_child: false,
            resize_end_child: true,
        });
        split.set_start_child(this._buildBuilderPane());
        split.set_end_child(this._buildOutputPane());
        split.set_position(620);
        toolbarView.set_content(split);
        this.set_content(toolbarView);
    }

    _buildBuilderPane() {
        const scroller = new Gtk.ScrolledWindow({
            hscrollbar_policy: Gtk.PolicyType.NEVER,
            min_content_width: 600,
        });
        const main = new Gtk.Box({
            orientation: Gtk.Orientation.VERTICAL,
            spacing: 14,
            margin_top: 16,
            margin_bottom: 16,
            margin_start: 16,
            margin_end: 12,
        });
        scroller.set_child(main);

        const sourceGroup = new Adw.PreferencesGroup({
            title: 'Source',
            description: 'For now this is a test transcript input. A recording source can be added as another stage later.',
        });
        main.append(sourceGroup);

        this._workflowNameRow = new Adw.EntryRow({
            title: 'Workflow Name',
            text: 'dictation-workflow',
        });
        sourceGroup.add(this._workflowNameRow);

        this._outputDirRow = new Adw.EntryRow({
            title: 'Output Folder',
            text: defaultOutputDir(),
        });
        sourceGroup.add(this._outputDirRow);

        this._transcriptBuffer = new Gtk.TextBuffer();
        const transcriptView = makeTextView(this._transcriptBuffer, true, true);
        const transcriptFrame = new Gtk.ScrolledWindow({
            min_content_height: 150,
            max_content_height: 240,
            hscrollbar_policy: Gtk.PolicyType.NEVER,
            child: transcriptView,
        });
        transcriptFrame.add_css_class('card');
        sourceGroup.add(transcriptFrame);

        const actionGroup = new Adw.PreferencesGroup({ title: 'Actions' });
        main.append(actionGroup);

        const runRow = new Adw.ActionRow({
            title: 'Run Workflow',
            subtitle: 'Runs enabled stages sequentially and writes prompt/output files.',
        });
        this._runButton = makeButton('Run', () => this._runWorkflow(), 'suggested-action');
        this._stopButton = makeButton('Stop', () => this._stopWorkflow(), 'destructive-action');
        this._stopButton.sensitive = false;
        runRow.add_suffix(this._stopButton);
        runRow.add_suffix(this._runButton);
        runRow.activatable_widget = this._runButton;
        actionGroup.add(runRow);

        const saveRow = new Adw.ActionRow({
            title: 'Workflow Preset',
            subtitle: statePath(),
        });
        saveRow.add_suffix(makeButton('Save', () => this._saveState()));
        saveRow.add_suffix(makeButton('Reset', () => this._setWorkflow(defaultWorkflow())));
        actionGroup.add(saveRow);

        const modelRow = new Adw.ActionRow({
            title: 'Installed Ollama Models',
            subtitle: 'Loading',
        });
        this._modelsRow = modelRow;
        modelRow.add_suffix(makeButton('Refresh', () => this._refreshModels()));
        actionGroup.add(modelRow);

        const addGroup = new Adw.PreferencesGroup({ title: 'Add Stage' });
        main.append(addGroup);
        const addRow = new Adw.ActionRow({
            title: 'Stage Templates',
            subtitle: 'Stages pipe their output into the next enabled stage.',
        });
        addRow.add_suffix(makeButton('Fix', () => this._addStageFromTemplate('fix')));
        addRow.add_suffix(makeButton('Vocab Search', () => this._addStageFromTemplate('search')));
        addRow.add_suffix(makeButton('Final', () => this._addStageFromTemplate('final')));
        addRow.add_suffix(makeButton('Custom', () => this._addStageFromTemplate('custom')));
        addGroup.add(addRow);

        this._stageGroup = new Adw.PreferencesGroup({
            title: 'Workflow',
            description: 'Placeholders: {input}, {transcript}, {previous}, {outputs}, {stage:Stage Name}.',
        });
        main.append(this._stageGroup);
        this._stageBox = new Gtk.Box({
            orientation: Gtk.Orientation.VERTICAL,
            spacing: 12,
            margin_top: 8,
        });
        this._stageGroup.add(this._stageBox);

        return scroller;
    }

    _buildOutputPane() {
        const wrapper = new Gtk.Box({
            orientation: Gtk.Orientation.VERTICAL,
            spacing: 10,
            margin_top: 16,
            margin_bottom: 16,
            margin_start: 12,
            margin_end: 16,
        });

        this._outputTitle = new Gtk.Label({
            label: 'Run Output',
            xalign: 0,
        });
        this._outputTitle.add_css_class('title-2');
        wrapper.append(this._outputTitle);

        this._statusLabel = new Gtk.Label({
            label: 'Ready',
            xalign: 0,
            wrap: true,
        });
        this._statusLabel.add_css_class('dim-label');
        wrapper.append(this._statusLabel);

        const scroller = new Gtk.ScrolledWindow({
            hscrollbar_policy: Gtk.PolicyType.NEVER,
            vexpand: true,
        });
        this._outputBox = new Gtk.Box({
            orientation: Gtk.Orientation.VERTICAL,
            spacing: 12,
        });
        scroller.set_child(this._outputBox);
        wrapper.append(scroller);

        return wrapper;
    }

    _loadState() {
        const state = readJsonFile(statePath()) ?? defaultWorkflow();
        this._setWorkflow(state);
    }

    _saveState() {
        const state = this._collectState();
        writeJsonFile(statePath(), state);
        this._setStatus(`Saved workflow to ${statePath()}`);
    }

    _collectState() {
        return {
            workflowName: entryText(this._workflowNameRow) || 'dictation-workflow',
            outputDir: entryText(this._outputDirRow) || defaultOutputDir(),
            transcript: textFromBuffer(this._transcriptBuffer),
            stages: this._stages.map(stage => this._stageFromWidgets(stage)),
        };
    }

    _setWorkflow(state) {
        this._workflowNameRow.text = state.workflowName || 'dictation-workflow';
        this._outputDirRow.text = state.outputDir || defaultOutputDir();
        setBufferText(this._transcriptBuffer, state.transcript || '');
        this._stages = [];
        this._renderStages();

        const stages = Array.isArray(state.stages) && state.stages.length > 0
            ? state.stages
            : defaultWorkflow().stages;
        for (const stage of stages)
            this._addStage(stage, false);

        this._setStatus('Workflow loaded');
    }

    async _refreshModels() {
        try {
            const output = await runCommand([cliPath(), 'ollama-list', '--json']);
            const data = JSON.parse(output);
            this._installedModels = (data.models ?? []).map(model => model.name);
            this._modelsRow.subtitle = this._installedModels.length > 0
                ? this._installedModels.join(', ')
                : 'No Ollama models found';
        } catch (error) {
            this._modelsRow.subtitle = error.message;
        }
    }

    _addStageFromTemplate(name) {
        this._addStage({ ...DEFAULT_PROMPTS[name] });
        this._saveState();
    }

    _addStage(stage, scroll = true) {
        const normalized = {
            id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
            name: stage.name || 'Custom stage',
            model: stage.model || 'granite4.1:3b',
            inputMode: stage.inputMode || 'previous',
            prompt: stage.prompt || DEFAULT_PROMPTS.custom.prompt,
            enabled: stage.enabled !== false,
        };
        this._stages.push(normalized);
        this._renderStages();
        if (scroll)
            this._setStatus(`Added stage ${normalized.name}`);
    }

    _renderStages() {
        while (this._stageBox.get_first_child())
            this._stageBox.remove(this._stageBox.get_first_child());
        this._stageWidgets.clear();

        this._stages.forEach((stage, index) => {
            if (index > 0) {
                const arrow = new Gtk.Label({
                    label: '->',
                    xalign: 0.5,
                });
                arrow.add_css_class('dim-label');
                this._stageBox.append(arrow);
            }
            this._stageBox.append(this._buildStageCard(stage, index));
        });
    }

    _buildStageCard(stage, index) {
        const card = new Gtk.Box({
            orientation: Gtk.Orientation.VERTICAL,
            spacing: 8,
            margin_top: 2,
            margin_bottom: 2,
            margin_start: 2,
            margin_end: 2,
        });
        card.add_css_class('card');

        const header = new Gtk.Box({
            orientation: Gtk.Orientation.HORIZONTAL,
            spacing: 6,
            margin_top: 10,
            margin_start: 10,
            margin_end: 10,
        });
        card.append(header);

        const title = new Gtk.Label({
            label: `Stage ${index + 1}`,
            xalign: 0,
            hexpand: true,
        });
        title.add_css_class('heading');
        header.append(title);

        const enabled = new Gtk.Switch({
            active: stage.enabled,
            valign: Gtk.Align.CENTER,
        });
        header.append(enabled);
        header.append(makeIconButton('go-up-symbolic', 'Move up', () => this._moveStage(stage.id, -1)));
        header.append(makeIconButton('go-down-symbolic', 'Move down', () => this._moveStage(stage.id, 1)));
        header.append(makeIconButton('user-trash-symbolic', 'Delete stage', () => this._removeStage(stage.id)));

        const nameRow = new Adw.EntryRow({
            title: 'Name',
            text: stage.name,
        });
        card.append(nameRow);

        const modelRow = new Adw.EntryRow({
            title: 'Ollama Model',
            text: stage.model,
        });
        card.append(modelRow);

        const inputRow = new Adw.ComboRow({
            title: 'Input',
            model: makeStringList(INPUT_OPTIONS),
            selected: Math.max(0, INPUT_OPTIONS.findIndex(([value]) => value === stage.inputMode)),
        });
        card.append(inputRow);

        const promptBuffer = new Gtk.TextBuffer();
        setBufferText(promptBuffer, stage.prompt);
        const promptFrame = new Gtk.ScrolledWindow({
            min_content_height: 130,
            max_content_height: 260,
            hscrollbar_policy: Gtk.PolicyType.NEVER,
            child: makeTextView(promptBuffer, true, false),
            margin_bottom: 10,
            margin_start: 10,
            margin_end: 10,
        });
        promptFrame.add_css_class('card');
        card.append(promptFrame);

        this._stageWidgets.set(stage.id, {
            enabled,
            nameRow,
            modelRow,
            inputRow,
            promptBuffer,
        });

        return card;
    }

    _stageFromWidgets(stage) {
        const widgets = this._stageWidgets.get(stage.id);
        if (!widgets)
            return stage;

        return {
            ...stage,
            enabled: widgets.enabled.active,
            name: entryText(widgets.nameRow) || stage.name,
            model: entryText(widgets.modelRow) || stage.model,
            inputMode: selectedOption(INPUT_OPTIONS, widgets.inputRow),
            prompt: textFromBuffer(widgets.promptBuffer),
        };
    }

    _moveStage(id, direction) {
        const index = this._stages.findIndex(stage => stage.id === id);
        const next = index + direction;
        if (index < 0 || next < 0 || next >= this._stages.length)
            return;

        this._stages = this._stages.map(stage => this._stageFromWidgets(stage));
        const [stage] = this._stages.splice(index, 1);
        this._stages.splice(next, 0, stage);
        this._renderStages();
        this._saveState();
    }

    _removeStage(id) {
        this._stages = this._stages
            .map(stage => this._stageFromWidgets(stage))
            .filter(stage => stage.id !== id);
        this._renderStages();
        this._saveState();
    }

    _clearOutputs() {
        while (this._outputBox.get_first_child())
            this._outputBox.remove(this._outputBox.get_first_child());
    }

    _setStatus(status) {
        this._statusLabel.label = status;
    }

    _setBusy(busy) {
        this._running = busy;
        this._runButton.sensitive = !busy;
        this._stopButton.sensitive = busy;
        this._saveHeaderButton.sensitive = !busy;
    }

    _stopWorkflow() {
        if (this._currentProcess)
            this._currentProcess.force_exit();
        this._setStatus('Stopping current stage');
    }

    async _runWorkflow() {
        if (this._running)
            return;

        const state = this._collectState();
        if (!state.transcript.trim()) {
            this._setStatus('Paste a transcript first');
            return;
        }

        const stages = state.stages.filter(stage => stage.enabled);
        if (stages.length === 0) {
            this._setStatus('Enable at least one stage');
            return;
        }

        this._saveState();
        this._clearOutputs();
        this._setBusy(true);

        const runDir = GLib.build_filenamev([
            state.outputDir,
            `${timestamp()}-${safeFilePart(state.workflowName, 'workflow')}`,
        ]);
        this._currentRunDir = runDir;
        GLib.mkdir_with_parents(runDir, 0o755);
        writeJsonFile(GLib.build_filenamev([runDir, 'workflow.json']), state);
        writeTextFile(GLib.build_filenamev([runDir, '000-transcript.txt']), state.transcript);

        let previous = state.transcript;
        const outputs = [];
        const outputsByName = new Map();
        const startedUsec = GLib.get_monotonic_time();

        try {
            for (let index = 0; index < stages.length; index += 1) {
                const stage = stages[index];
                const input = stage.inputMode === 'transcript' ? state.transcript : previous;
                const outputsText = outputs
                    .map(output => `## ${output.name}\n${output.text}`)
                    .join('\n\n');
                const prompt = renderPrompt(stage.prompt, {
                    input,
                    transcript: state.transcript,
                    previous,
                    outputsText,
                    outputsByName,
                });
                const prefix = `${String(index + 1).padStart(3, '0')}-${safeFilePart(stage.name, 'stage')}`;
                const promptPath = GLib.build_filenamev([runDir, `${prefix}.prompt.txt`]);
                const outputPath = GLib.build_filenamev([runDir, `${prefix}.output.txt`]);
                writeTextFile(promptPath, prompt);

                this._setStatus(`Running ${stage.name} with ${stage.model}`);
                this._addStageOutputCard({
                    stage,
                    promptPath,
                    outputPath,
                    prompt,
                    output: 'Running...',
                    elapsedMs: null,
                });

                const stageStart = GLib.get_monotonic_time();
                const output = await this._runOllama(stage.model, prompt);
                const elapsedMs = Math.round((GLib.get_monotonic_time() - stageStart) / 1000);
                writeTextFile(outputPath, output);
                previous = output;
                outputs.push({ name: stage.name, text: output });
                outputsByName.set(stage.name, output);
                this._replaceLastStageOutput({
                    stage,
                    promptPath,
                    outputPath,
                    prompt,
                    output,
                    elapsedMs,
                });
            }

            const totalMs = Math.round((GLib.get_monotonic_time() - startedUsec) / 1000);
            writeJsonFile(GLib.build_filenamev([runDir, 'summary.json']), {
                workflowName: state.workflowName,
                runDir,
                elapsedMs: totalMs,
                stages: stages.map(stage => stage.name),
            });
            this._setStatus(`Finished workflow in ${Math.round(totalMs / 1000)}s. Output: ${runDir}`);
        } catch (error) {
            writeTextFile(GLib.build_filenamev([runDir, 'error.txt']), error.message);
            this._setStatus(`Workflow stopped or failed: ${error.message}`);
        } finally {
            this._currentProcess = null;
            this._setBusy(false);
        }
    }

    async _runOllama(model, prompt) {
        const process = Gio.Subprocess.new(
            ['ollama', 'run', '--nowordwrap', '--hidethinking', model],
            Gio.SubprocessFlags.STDIN_PIPE |
                Gio.SubprocessFlags.STDOUT_PIPE |
                Gio.SubprocessFlags.STDERR_PIPE
        );
        this._currentProcess = process;
        const [stdout, stderr] = await process.communicate_utf8_async(prompt, null);
        this._currentProcess = null;

        if (!process.get_successful())
            throw new Error((stderr ?? '').trim() || `ollama run ${model} failed`);

        return (stdout ?? '').trim();
    }

    _addStageOutputCard(data) {
        this._outputBox.append(this._buildOutputCard(data));
    }

    _replaceLastStageOutput(data) {
        const last = this._outputBox.get_last_child();
        if (last)
            this._outputBox.remove(last);
        this._outputBox.append(this._buildOutputCard(data));
    }

    _buildOutputCard({ stage, promptPath, outputPath, prompt, output, elapsedMs }) {
        const card = new Gtk.Box({
            orientation: Gtk.Orientation.VERTICAL,
            spacing: 8,
            margin_top: 2,
            margin_bottom: 2,
            margin_start: 2,
            margin_end: 2,
        });
        card.add_css_class('card');

        const header = new Gtk.Box({
            orientation: Gtk.Orientation.VERTICAL,
            spacing: 3,
            margin_top: 12,
            margin_start: 12,
            margin_end: 12,
        });
        card.append(header);

        const title = new Gtk.Label({
            label: stage.name,
            xalign: 0,
            ellipsize: Pango.EllipsizeMode.END,
        });
        title.add_css_class('heading');
        header.append(title);

        const subtitle = [
            stage.model,
            elapsedMs === null ? 'running' : `${elapsedMs}ms`,
            outputPath,
        ].join(' | ');
        const meta = new Gtk.Label({
            label: subtitle,
            xalign: 0,
            wrap: true,
        });
        meta.add_css_class('dim-label');
        header.append(meta);

        const promptBuffer = new Gtk.TextBuffer();
        setBufferText(promptBuffer, `Prompt file:\n${promptPath}\n\n${prompt ?? ''}`);
        const promptView = makeTextView(promptBuffer, false, true);
        const promptFrame = new Gtk.ScrolledWindow({
            min_content_height: 100,
            max_content_height: 210,
            hscrollbar_policy: Gtk.PolicyType.NEVER,
            margin_start: 12,
            margin_end: 12,
            child: promptView,
        });
        promptFrame.add_css_class('view');
        card.append(promptFrame);

        const outputBuffer = new Gtk.TextBuffer();
        setBufferText(outputBuffer, output);
        const outputFrame = new Gtk.ScrolledWindow({
            min_content_height: 220,
            max_content_height: 460,
            hscrollbar_policy: Gtk.PolicyType.NEVER,
            margin_bottom: 12,
            margin_start: 12,
            margin_end: 12,
            child: makeTextView(outputBuffer, false, true),
        });
        outputFrame.add_css_class('view');
        card.append(outputFrame);

        const compact = new Gtk.Label({
            label: compactText(output),
            xalign: 0,
            wrap: true,
            margin_bottom: 12,
            margin_start: 12,
            margin_end: 12,
        });
        compact.add_css_class('caption');
        compact.add_css_class('dim-label');
        card.append(compact);

        return card;
    }
});

const app = new Adw.Application({
    application_id: 'dev.local.Chirper.WorkflowBuilder',
    flags: Gio.ApplicationFlags.FLAGS_NONE,
});

app.connect('activate', application => {
    window = new WorkflowBuilderWindow(application);
    window.present();
});

app.run([GLib.get_prgname() ?? 'chirper-workflow-builder']);
