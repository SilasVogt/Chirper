import Adw from 'gi://Adw';
import Gdk from 'gi://Gdk';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Gtk from 'gi://Gtk';

Gio._promisify(
    Gio.Subprocess.prototype,
    'communicate_utf8_async',
    'communicate_utf8_finish'
);

const WHISPER_MODELS = ['medium', 'large-v3-turbo', 'large-v3'];
const RECOMMENDED_WHISPER_MODEL = 'large-v3-turbo';
const OLLAMA_MODELS = ['granite4.1:3b', 'granite4.1:8b', 'olmo2:7b'];
const CODEX_MODEL = 'gpt-5.4';
const CODEX_EFFORT = 'medium';
const EXTENSION_UUID = 'chirper@local';
const EXTENSION_SCHEMA_ID = 'org.gnome.shell.extensions.chirper';
const TOGGLE_RECORDING_KEY = 'toggle-recording';
const CHECK_UPDATES_KEY = 'check-updates';
const DAEMON_SERVICE = 'chirper-daemon.service';
const DEFAULT_TOGGLE_SHORTCUT = '<Ctrl><Alt>space';
const SHORTCUT_OPTIONS = [
    DEFAULT_TOGGLE_SHORTCUT,
    '<Super>space',
    '<Ctrl><Alt>r',
    '<Ctrl><Shift>space',
    'custom',
];
const EXTENSION_FORMATTING_PROMPT = 'extension=Your job is to fix transcription errors and human made mistakes. the user may misspeak and try to correct themselves or specify specific spellings of words and names. Return only the cleaned-up final text. Apply spoken edit commands, punctuation, casing, spelling, URLs, emails, basic markdown and identifiers. Do not explain your actions.\n\n{raw}';
const TEST_PROMPT = `Hello Chirper. I need to write down accent-friendly words. This is a bullet point list with title Accent Friendly Words: water, tomato, schedule, data, router, aluminium, privacy. End of list.

Please write an email to Maya comma subject colon quarterly update period The meeting moved to Thursday at 9:30 AM comma the budget is $12,450 comma and the website is chirper dot local slash launch period

In the deployment notes, mention that systemd keeps the Chirper services running, and we should also look at: PostgreSQL, FFmpeg, GNOME, Nextcloud, and Tailscale. Finish with thanks exclamation mark`;

let window = null;
let cssInstalled = false;

const ONBOARDING_CSS = `
.comparison-card {
  background-color: @view_bg_color;
  border: 4px solid alpha(@borders, 0.45);
  border-radius: 8px;
}

.comparison-card-selected {
  border: 4px solid @accent_bg_color;
  background-color: alpha(@accent_bg_color, 0.12);
}

.selection-required {
  border: 2px solid @warning_color;
  border-radius: 8px;
  background-color: alpha(@warning_color, 0.10);
}
`;

function installCss() {
    if (cssInstalled)
        return;

    const provider = new Gtk.CssProvider();
    provider.load_from_data(ONBOARDING_CSS, -1);
    Gtk.StyleContext.add_provider_for_display(
        Gdk.Display.get_default(),
        provider,
        Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
    );
    cssInstalled = true;
}

function repoRoot() {
    try {
        const [path] = GLib.filename_from_uri(import.meta.url);
        return GLib.path_get_dirname(GLib.path_get_dirname(GLib.path_get_dirname(path)));
    } catch (_error) {
        return null;
    }
}

function installedExtensionDir() {
    return GLib.build_filenamev([GLib.get_user_data_dir(), 'gnome-shell', 'extensions', EXTENSION_UUID]);
}

function extensionSchemaDirs() {
    const dirs = [];
    const root = repoRoot();
    if (root)
        dirs.push(GLib.build_filenamev([root, 'extensions', 'gnome', EXTENSION_UUID, 'schemas']));

    dirs.push(GLib.build_filenamev([installedExtensionDir(), 'schemas']));
    return dirs;
}

function loadExtensionSettings() {
    for (const dir of extensionSchemaDirs()) {
        if (!GLib.file_test(GLib.build_filenamev([dir, 'gschemas.compiled']), GLib.FileTest.EXISTS))
            continue;

        try {
            const source = Gio.SettingsSchemaSource.new_from_directory(
                dir,
                Gio.SettingsSchemaSource.get_default(),
                false
            );
            const schema = source.lookup(EXTENSION_SCHEMA_ID, true);
            if (schema)
                return new Gio.Settings({settings_schema: schema});
        } catch (error) {
            console.debug(`failed to load extension settings from ${dir}: ${error.message}`);
        }
    }

    return null;
}

function formatAccelerator(value) {
    if (!value)
        return '';

    return value
        .replace(/</g, '')
        .replace(/>/g, '+')
        .replace(/\+$/g, '')
        .replace(/\bspace\b/i, 'Space')
        .replace(/\bctrl\b/i, 'Ctrl')
        .replace(/\balt\b/i, 'Alt')
        .replace(/\bsuper\b/i, 'Super')
        .replace(/\bshift\b/i, 'Shift');
}

function shortcutLabel(value) {
    if (value === 'custom')
        return 'Custom shortcut';

    if (value === DEFAULT_TOGGLE_SHORTCUT)
        return `${formatAccelerator(value)} (Default)`;

    return formatAccelerator(value);
}

function cliPath() {
    const configured = GLib.getenv('CHIRPER_CLI');
    if (configured)
        return configured;

    const root = repoRoot();
    if (root) {
        const debugCli = GLib.build_filenamev([root, 'target', 'debug', 'chirper']);
        if (GLib.file_test(debugCli, GLib.FileTest.IS_EXECUTABLE))
            return debugCli;

        const releaseCli = GLib.build_filenamev([root, 'target', 'release', 'chirper']);
        if (GLib.file_test(releaseCli, GLib.FileTest.IS_EXECUTABLE))
            return releaseCli;
    }

    return 'chirper';
}

function runtimeStatePath(name = 'onboarding-record-state') {
    const runtimeDir = GLib.getenv('XDG_RUNTIME_DIR') || GLib.get_tmp_dir();
    return GLib.build_filenamev([runtimeDir, 'chirper', name]);
}

function configDir() {
    return GLib.build_filenamev([GLib.get_user_config_dir(), 'chirper']);
}

function configPath() {
    return GLib.build_filenamev([configDir(), 'config.toml']);
}

function configFileExists() {
    return GLib.file_test(configPath(), GLib.FileTest.EXISTS);
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

async function runCli(args) {
    return await runCommand([cliPath(), ...args]);
}

async function runCliJson(args) {
    return JSON.parse(await runCli(args));
}

function runDetached(argv) {
    Gio.Subprocess.new(
        argv,
        Gio.SubprocessFlags.STDOUT_SILENCE | Gio.SubprocessFlags.STDERR_SILENCE
    );
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

function makeStringList(labels) {
    const list = new Gtk.StringList();
    for (const label of labels)
        list.append(label);
    return list;
}

function statusLabel(label, cssClass = null) {
    const widget = new Gtk.Label({
        label,
        valign: Gtk.Align.CENTER,
    });

    if (cssClass)
        widget.add_css_class(cssClass);

    return widget;
}

function setStatusLabel(widget, label, cssClass = null) {
    widget.label = label;
    for (const name of ['success', 'warning', 'error'])
        widget.remove_css_class(name);

    if (cssClass)
        widget.add_css_class(cssClass);
}

function textView(text, options = {}) {
    const buffer = new Gtk.TextBuffer();
    buffer.set_text(text ?? '', -1);
    const view = new Gtk.TextView({
        buffer,
        editable: false,
        cursor_visible: false,
        monospace: options.monospace ?? false,
        wrap_mode: Gtk.WrapMode.WORD_CHAR,
        top_margin: 12,
        bottom_margin: 12,
        left_margin: 12,
        right_margin: 12,
    });
    view.add_css_class('view');

    return new Gtk.ScrolledWindow({
        min_content_height: options.minHeight ?? 110,
        max_content_height: options.maxHeight ?? 210,
        hscrollbar_policy: Gtk.PolicyType.NEVER,
        child: view,
    });
}

function textBlock(text) {
    const box = new Gtk.Box({
        orientation: Gtk.Orientation.VERTICAL,
        margin_top: 12,
        margin_bottom: 12,
        margin_start: 12,
        margin_end: 12,
    });
    box.add_css_class('view');

    const label = new Gtk.Label({
        label: text,
        selectable: true,
        wrap: true,
        xalign: 0,
        yalign: 0,
    });
    box.append(label);
    return box;
}

function compact(text, limit = 140) {
    const value = String(text ?? '').replace(/\s+/g, ' ').trim();
    if (value.length <= limit)
        return value || 'No output';

    return `${value.slice(0, limit - 1)}...`;
}

function formatElapsed(milliseconds) {
    const value = Number(milliseconds);
    if (!Number.isFinite(value))
        return null;
    if (value >= 1000)
        return `${(value / 1000).toFixed(2)}s`;

    return `${Math.round(value)}ms`;
}

function formatPercent(value) {
    const number = Number(value);
    if (!Number.isFinite(number))
        return null;

    return `${number.toFixed(1)}%`;
}

function formatBytes(bytes) {
    const value = Number(bytes);
    if (!Number.isFinite(value))
        return null;

    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let scaled = value;
    let unit = 0;
    while (scaled >= 1000 && unit < units.length - 1) {
        scaled /= 1000;
        unit += 1;
    }

    const precision = scaled >= 10 || unit === 0 ? 0 : 1;
    return `${scaled.toFixed(precision)} ${units[unit]}`;
}

function metricsSummary(elapsedMs, metrics) {
    const parts = [];
    const elapsed = formatElapsed(elapsedMs);
    if (elapsed)
        parts.push(`Runtime ${elapsed}`);

    if (metrics?.samples > 0) {
        const cpu = formatPercent(metrics.avg_cpu_percent);
        if (cpu)
            parts.push(`CPU ${cpu} avg`);

        const ram = formatBytes(metrics.avg_ram_used_bytes);
        if (ram)
            parts.push(`RAM ${ram} avg`);

        const gpu = formatPercent(metrics.avg_gpu_percent);
        if (gpu)
            parts.push(`GPU ${gpu} avg`);

        const vramUsed = formatBytes(metrics.avg_vram_used_bytes);
        if (vramUsed) {
            const vramTotal = formatBytes(metrics.vram_total_bytes);
            parts.push(`VRAM ${vramTotal ? `${vramUsed} / ${vramTotal}` : vramUsed} avg`);
        }
    } else if (metrics && elapsed) {
        parts.push('resource telemetry unavailable');
    }

    return parts.join(' | ') || 'Measurement unavailable';
}

function whisperInfo(checks, model) {
    return checks?.whisper_models?.find(entry => entry.name === model) ?? null;
}

function whisperLabel(model) {
    return model === RECOMMENDED_WHISPER_MODEL ? `${model} (Recommended)` : model;
}

function ollamaInfo(checks, model) {
    return checks?.ollama_models?.find(entry => entry.name === model) ?? null;
}

function commandInfo(checks, name) {
    return checks?.commands?.[name] ?? {};
}

function modelLabel(model) {
    if (model === 'granite4.1:3b')
        return 'Granite 3B';
    if (model === 'granite4.1:8b')
        return 'Granite 8B';
    if (model === 'olmo2:7b')
        return 'OLMo 2 7B';

    return model;
}

function modelNote(model) {
    if (model === 'granite4.1:3b')
        return 'Less smart model for devices with up to 8 GB of VRAM.';
    if (model === 'granite4.1:8b')
        return 'Pretty smart at text formatting, even with several tasks combined. Needs at least 12 GB of VRAM.';
    if (model === 'olmo2:7b')
        return 'Mostly smart model for around 8 GB of VRAM. Does a good job, but may miss things if several tasks are combined in one recording.';

    return null;
}

function modelChoiceLabel(model) {
    return `${modelLabel(model)} (${model})`;
}

function formatterIdForOllama(model) {
    return `ollama:${model}`;
}

function parseFormatterId(id) {
    if (id?.startsWith('ollama:'))
        return { type: 'ollama', model: id.slice('ollama:'.length) };
    if (id === 'codex')
        return { type: 'codex', model: CODEX_MODEL };

    return { type: 'unknown', model: null };
}

const OnboardingWindow = GObject.registerClass(class OnboardingWindow extends Adw.ApplicationWindow {
    constructor(application) {
        super({
            application,
            title: 'Welcome to Chirper',
            default_width: 1800,
            default_height: 1000,
        });

        this._step = 0;
        this._checks = null;
        this._setupRows = [];
        this._whisperRows = [];
        this._formatRows = [];
        this._saveRows = [];
        this._whisperResults = new Map();
        this._formatResults = new Map();
        this._selectedWhisper = null;
        this._selectedFormatter = null;
        this._recommendationSaved = false;
        this._updatingRecommendationControls = false;
        this._extensionSettings = loadExtensionSettings();
        this._recording = false;
        this._recordingPath = null;
        this._formatRetryRecording = false;
        this._formatRetryRecordingPath = null;
        this._build();
        this._refreshChecks();
        this._refreshIntegrationStatus();
    }

    _build() {
        const toolbarView = new Adw.ToolbarView();
        const header = new Adw.HeaderBar({
            title_widget: new Adw.WindowTitle({
                title: 'Welcome to Chirper',
                subtitle: 'Set up dictation, transcription, and formatting',
            }),
        });
        toolbarView.add_top_bar(header);

        this._stack = new Gtk.Stack({
            transition_type: Gtk.StackTransitionType.SLIDE_LEFT_RIGHT,
            vexpand: true,
        });
        this._stack.add_named(this._buildSetupPage(), 'setup');
        this._stack.add_named(this._buildRecordPage(), 'record');
        this._stack.add_named(this._buildFormatPage(), 'format');
        this._stack.add_named(this._buildSavePage(), 'save');

        const footer = new Gtk.Box({
            orientation: Gtk.Orientation.HORIZONTAL,
            spacing: 10,
            margin_top: 12,
            margin_bottom: 12,
            margin_start: 18,
            margin_end: 18,
        });
        this._stepLabel = new Gtk.Label({
            label: 'Step 1 of 4',
            xalign: 0,
            hexpand: true,
        });
        this._backButton = makeButton('Back', () => this._goBack());
        this._nextButton = makeButton('Next', () => this._handleNext(), 'suggested-action');
        footer.append(this._stepLabel);
        footer.append(this._backButton);
        footer.append(this._nextButton);

        const content = new Gtk.Box({
            orientation: Gtk.Orientation.VERTICAL,
            vexpand: true,
        });
        content.append(this._stack);
        content.append(footer);

        toolbarView.set_content(content);
        this.set_content(toolbarView);
        this._syncNavigation();
    }

    _page(title, subtitle) {
        const scroller = new Gtk.ScrolledWindow({
            hscrollbar_policy: Gtk.PolicyType.NEVER,
            vexpand: true,
        });
        const clamp = new Adw.Clamp({
            maximum_size: 1720,
            tightening_threshold: 1100,
            margin_top: 20,
            margin_bottom: 20,
            margin_start: 18,
            margin_end: 18,
        });
        const box = new Gtk.Box({
            orientation: Gtk.Orientation.VERTICAL,
            spacing: 16,
        });
        const titleLabel = new Gtk.Label({
            label: title,
            xalign: 0,
            wrap: true,
        });
        titleLabel.add_css_class('title-1');
        const subtitleLabel = new Gtk.Label({
            label: subtitle,
            xalign: 0,
            wrap: true,
        });
        subtitleLabel.add_css_class('dim-label');

        box.append(titleLabel);
        box.append(subtitleLabel);
        clamp.set_child(box);
        scroller.set_child(clamp);

        return { scroller, box };
    }

    _buildSetupPage() {
        const { scroller, box } = this._page(
            'Welcome to Chirper',
            'First, Chirper checks the tools and models needed for the guided setup. Missing models can be installed here before testing.'
        );

        this._setupGroup = new Adw.PreferencesGroup({
            title: 'Setup Checks',
            description: 'These checks come from `chirper onboarding-check --json` so other desktop UIs can reuse the same contract.',
        });
        box.append(this._setupGroup);

        const optionsGroup = new Adw.PreferencesGroup({ title: 'Comparison Options' });
        this._includeCodexRow = new Adw.SwitchRow({
            title: 'Include Codex in Formatting Test',
            subtitle: `Uses ${CODEX_MODEL} at ${CODEX_EFFORT} effort if the Codex CLI is available.`,
            active: false,
        });
        optionsGroup.add(this._includeCodexRow);
        box.append(optionsGroup);

        const actionGroup = new Adw.PreferencesGroup();
        const refreshRow = new Adw.ActionRow({
            title: 'Refresh Checks',
            subtitle: 'Re-run command and model discovery.',
        });
        this._refreshChecksButton = makeButton('Refresh', () => this._refreshChecks());
        refreshRow.add_suffix(this._refreshChecksButton);
        refreshRow.activatable_widget = this._refreshChecksButton;
        actionGroup.add(refreshRow);
        box.append(actionGroup);

        return scroller;
    }

    _buildRecordPage() {
        const { scroller, box } = this._page(
            'Choose a Whisper model',
            'Record the paragraph once. This step only compares Whisper transcription; if your whisper.cpp build uses Vulkan or ROCm, the GPU can still be active.'
        );

        const promptGroup = new Adw.PreferencesGroup({
            title: 'Read This Aloud',
            description: 'Read naturally. The text includes accent-sensitive words, a list, punctuation, casing, numbers, a URL, and technical spelling.',
        });
        promptGroup.add(textBlock(TEST_PROMPT));
        box.append(promptGroup);

        const recordGroup = new Adw.PreferencesGroup({ title: 'Recording' });
        this._recordStatusRow = new Adw.ActionRow({
            title: 'Record Test Audio',
            subtitle: 'Ready',
        });
        this._recordButton = makeButton('Record', () => this._toggleRecording(), 'suggested-action');
        recordGroup.add(this._recordStatusRow);
        this._recordStatusRow.add_suffix(this._recordButton);
        this._recordStatusRow.activatable_widget = this._recordButton;
        box.append(recordGroup);

        this._whisperGroup = new Adw.PreferencesGroup({
            title: 'Whisper Results',
            description: 'Pick the transcript that best matches what you read.',
        });
        box.append(this._whisperGroup);

        return scroller;
    }

    _buildFormatPage() {
        const { scroller, box } = this._page(
            'Choose formatting and corrections',
            'Chirper will pass your preferred transcript through the local formatting models one at a time. Codex is included only if you enabled it on the first step.'
        );

        const actionGroup = new Adw.PreferencesGroup({ title: 'Run Formatting Test' });
        this._formatStatusRow = new Adw.ActionRow({
            title: 'Formatting Test',
            subtitle: 'Waiting for a selected transcript.',
        });
        this._formatButton = makeButton('Run Test', () => this._runFormattingTests(), 'suggested-action');
        this._formatStatusRow.add_suffix(this._formatButton);
        this._formatStatusRow.activatable_widget = this._formatButton;
        actionGroup.add(this._formatStatusRow);
        box.append(actionGroup);

        this._retryPromptGroup = new Adw.PreferencesGroup({
            title: 'Try Another Prompt',
            description: 'Use this after the premade prompt if you want to compare the same models on your own wording.',
        });
        this._retryPromptGroup.visible = false;
        this._retryPromptRow = new Adw.ActionRow({
            title: 'Record a new prompt and compare again',
            subtitle: 'Keeps the Whisper model you already chose, transcribes this new recording once, then reruns the formatter models automatically.',
        });
        this._retryPromptButton = makeButton('Record New Prompt', () => this._toggleFormatRetryRecording(), 'suggested-action');
        this._retryPromptRow.add_suffix(this._retryPromptButton);
        this._retryPromptRow.activatable_widget = this._retryPromptButton;
        this._retryPromptGroup.add(this._retryPromptRow);
        box.append(this._retryPromptGroup);

        this._formatGroup = new Adw.PreferencesGroup({
            title: 'Formatted Outputs',
            description: 'Pick the output you would want Chirper to paste.',
        });
        box.append(this._formatGroup);

        return scroller;
    }

    _buildSavePage() {
        const { scroller, box } = this._page(
            'Save recommended config',
            'Review the choices from the test. You can save them directly, open the config folder, or remove unused onboarding models.'
        );

        this._recommendationGroup = new Adw.PreferencesGroup({ title: 'Recommendation' });
        box.append(this._recommendationGroup);

        this._fallbackGroup = new Adw.PreferencesGroup({ title: 'Codex Fallback' });
        this._fallbackRow = new Adw.ComboRow({
            title: 'Keep Local Fallback Model',
            subtitle: 'Used as the configured Ollama model if you later switch away from Codex.',
            model: makeStringList(OLLAMA_MODELS.map(modelChoiceLabel)),
            selected: 1,
        });
        this._fallbackRow.connect('notify::selected', () => this._markRecommendationDirty());
        this._fallbackGroup.add(this._fallbackRow);
        box.append(this._fallbackGroup);

        const cleanupGroup = new Adw.PreferencesGroup({ title: 'Cleanup' });
        this._removeUnusedRow = new Adw.SwitchRow({
            title: 'Offer to remove unused onboarding models after saving',
            subtitle: 'Only removes the recommended Whisper/Ollama models that were not selected. It leaves unrelated models alone.',
            active: false,
        });
        this._removeUnusedRow.connect('notify::active', () => this._markRecommendationDirty());
        cleanupGroup.add(this._removeUnusedRow);
        box.append(cleanupGroup);

        const integrationGroup = new Adw.PreferencesGroup({
            title: 'Desktop Integration',
            description: 'Check the GNOME extension and the user daemon that handle the shortcut and recording commands.',
        });
        this._extensionStatusRow = new Adw.ActionRow({
            title: 'GNOME Extension',
            subtitle: 'Checking',
        });
        this._extensionStatusLabel = statusLabel('Checking', 'warning');
        this._extensionStatusRow.add_suffix(this._extensionStatusLabel);
        integrationGroup.add(this._extensionStatusRow);

        this._serviceStatusRow = new Adw.ActionRow({
            title: 'systemd User Service',
            subtitle: 'Checking',
        });
        this._serviceStatusLabel = statusLabel('Checking', 'warning');
        this._serviceStatusRow.add_suffix(this._serviceStatusLabel);
        integrationGroup.add(this._serviceStatusRow);

        this._daemonStatusRow = new Adw.ActionRow({
            title: 'Chirper Daemon',
            subtitle: 'Checking',
        });
        this._daemonStatusLabel = statusLabel('Checking', 'warning');
        this._daemonStatusRow.add_suffix(this._daemonStatusLabel);
        integrationGroup.add(this._daemonStatusRow);

        const refreshIntegrationRow = new Adw.ActionRow({
            title: 'Refresh Integration Checks',
            subtitle: 'Recheck extension, service, and daemon status.',
        });
        this._refreshIntegrationButton = makeButton('Refresh', () => this._refreshIntegrationStatus());
        refreshIntegrationRow.add_suffix(this._refreshIntegrationButton);
        refreshIntegrationRow.activatable_widget = this._refreshIntegrationButton;
        integrationGroup.add(refreshIntegrationRow);
        box.append(integrationGroup);

        const extensionGroup = new Adw.PreferencesGroup({
            title: 'GNOME Extension Preferences',
            description: 'These settings are saved to the GNOME extension and apply after the extension reloads its settings.',
        });
        const currentShortcut = this._currentExtensionShortcut();
        const shortcutIndex = SHORTCUT_OPTIONS.includes(currentShortcut)
            ? SHORTCUT_OPTIONS.indexOf(currentShortcut)
            : SHORTCUT_OPTIONS.indexOf('custom');
        this._shortcutRow = new Adw.ComboRow({
            title: 'Record / Stop Shortcut',
            subtitle: 'Used by the GNOME Shell extension to start recording and stop recording.',
            model: makeStringList(SHORTCUT_OPTIONS.map(shortcutLabel)),
            selected: shortcutIndex,
        });
        this._shortcutRow.connect('notify::selected', () => {
            this._syncShortcutCustomRow();
            this._markRecommendationDirty();
        });
        extensionGroup.add(this._shortcutRow);

        this._customShortcutRow = new Adw.EntryRow({
            title: 'Custom Shortcut',
            text: currentShortcut,
        });
        this._customShortcutRow.connect('notify::text', () => this._markRecommendationDirty());
        extensionGroup.add(this._customShortcutRow);

        this._updateChecksRow = new Adw.ComboRow({
            title: 'Automatic Update Checks',
            subtitle: 'Check periodically and notify when the installed source checkout is behind upstream.',
            model: makeStringList(['Manual only', 'Check periodically and notify']),
            selected: this._extensionSettings?.get_boolean(CHECK_UPDATES_KEY) ? 1 : 0,
        });
        this._updateChecksRow.connect('notify::selected', () => this._markRecommendationDirty());
        extensionGroup.add(this._updateChecksRow);

        if (!this._extensionSettings) {
            this._shortcutRow.subtitle = 'GNOME extension settings schema unavailable.';
            this._shortcutRow.set_sensitive(false);
            this._customShortcutRow.set_sensitive(false);
            this._updateChecksRow.set_sensitive(false);
        }
        this._syncShortcutCustomRow();
        box.append(extensionGroup);

        const actionGroup = new Adw.PreferencesGroup();
        this._saveStatusRow = new Adw.ActionRow({
            title: 'Save Configuration',
            subtitle: 'Not saved yet',
        });
        this._openConfigButton = makeButton('Open Config Folder', () => this._openConfigFolder());
        this._saveButton = makeButton('Save', () => this._saveRecommendation(), 'suggested-action');
        this._saveStatusRow.add_suffix(this._openConfigButton);
        this._saveStatusRow.add_suffix(this._saveButton);
        this._saveStatusRow.activatable_widget = this._saveButton;
        actionGroup.add(this._saveStatusRow);
        box.append(actionGroup);

        return scroller;
    }

    _goBack() {
        this._setStep(Math.max(0, this._step - 1));
    }

    _goNext() {
        if (this._step === 2) {
            this._recommendationSaved = false;
            this._refreshRecommendation();
        }

        this._setStep(Math.min(3, this._step + 1));
    }

    _handleNext() {
        if (this._step === 3) {
            if (this._recommendationSaved) {
                this.close();
                return;
            }

            if (configFileExists()) {
                this._confirmDiscardUnsavedRecommendation();
            } else {
                this._saveStatusRow.subtitle = 'Save the recommended configuration before finishing.';
                this._syncNavigation();
            }
            return;
        }

        this._goNext();
    }

    _setStep(step) {
        this._step = step;
        this._stack.visible_child_name = ['setup', 'record', 'format', 'save'][step];
        this._syncNavigation();
    }

    _syncNavigation() {
        const recordingBusy = this._recording || this._formatRetryRecording;
        this._stepLabel.label = `Step ${this._step + 1} of 4`;
        this._backButton.sensitive = this._step > 0 && !recordingBusy;
        this._nextButton.label = this._step === 3 ? 'Done' : 'Next';
        if (this._step === 1 && !this._selectedWhisper)
            this._nextButton.label = 'Choose Transcript';
        if (this._step === 2 && !this._selectedFormatter)
            this._nextButton.label = 'Choose Output';
        if (this._step === 3 && !this._recommendationSaved && !configFileExists())
            this._nextButton.label = 'Save Required';
        this._nextButton.sensitive = !recordingBusy && this._canAdvance();
    }

    _canAdvance() {
        if (this._step === 1)
            return Boolean(this._selectedWhisper);
        if (this._step === 2)
            return Boolean(this._selectedFormatter);
        if (this._step === 3)
            return this._recommendationSaved || configFileExists();

        return true;
    }

    _clearRows(group, rows) {
        for (const row of rows)
            group.remove(row);

        rows.length = 0;
    }

    _addInfoRow(group, rows, title, subtitle = null) {
        const row = new Adw.ActionRow({ title, subtitle });
        row.set_sensitive(false);
        group.add(row);
        rows.push(row);
        return row;
    }

    _addSelectionRequiredRow(group, rows, title, subtitle) {
        const row = new Adw.ActionRow({ title, subtitle });
        row.add_css_class('selection-required');
        row.add_suffix(statusLabel('Required', 'warning'));
        group.add(row);
        rows.push(row);
        return row;
    }

    _markRecommendationDirty() {
        if (this._updatingRecommendationControls)
            return;
        if (this._step !== 3)
            return;

        this._recommendationSaved = false;
        if (this._saveStatusRow)
            this._saveStatusRow.subtitle = 'Unsaved changes';
        this._syncNavigation();
    }

    _confirmDiscardUnsavedRecommendation() {
        const dialog = new Adw.MessageDialog({
            transient_for: this,
            modal: true,
            heading: 'Recommended Configuration Not Saved',
            body: 'You have not saved the recommended configuration. If you close onboarding now, this recommendation will be lost.',
        });
        dialog.add_response('cancel', 'Go Back');
        dialog.add_response('discard', 'Close Without Saving');
        dialog.set_default_response('cancel');
        dialog.set_close_response('cancel');
        dialog.set_response_appearance('discard', Adw.ResponseAppearance.DESTRUCTIVE);
        dialog.connect('response', (_dialog, response) => {
            if (response === 'discard')
                this.close();
        });
        dialog.present();
    }

    _currentExtensionShortcut() {
        if (!this._extensionSettings)
            return DEFAULT_TOGGLE_SHORTCUT;

        const shortcuts = this._extensionSettings.get_strv(TOGGLE_RECORDING_KEY);
        return shortcuts[0] || DEFAULT_TOGGLE_SHORTCUT;
    }

    _selectedExtensionShortcut() {
        const selected = SHORTCUT_OPTIONS[this._shortcutRow.selected] ?? DEFAULT_TOGGLE_SHORTCUT;
        if (selected !== 'custom')
            return selected;

        return this._customShortcutRow.text.trim();
    }

    _syncShortcutCustomRow() {
        if (!this._customShortcutRow || !this._shortcutRow)
            return;

        const selected = SHORTCUT_OPTIONS[this._shortcutRow.selected] ?? DEFAULT_TOGGLE_SHORTCUT;
        this._customShortcutRow.visible = selected === 'custom';
    }

    async _refreshIntegrationStatus() {
        if (!this._extensionStatusRow)
            return;

        this._refreshIntegrationButton.sensitive = false;
        setStatusLabel(this._extensionStatusLabel, 'Checking', 'warning');
        setStatusLabel(this._serviceStatusLabel, 'Checking', 'warning');
        setStatusLabel(this._daemonStatusLabel, 'Checking', 'warning');

        try {
            const output = await runCommand(['gnome-extensions', 'info', EXTENSION_UUID]);
            const active = /^  State:\s+ACTIVE$/m.test(output);
            const enabled = /^  Enabled:\s+Yes$/m.test(output);
            if (active) {
                this._extensionStatusRow.subtitle = 'Installed, enabled, and active.';
                setStatusLabel(this._extensionStatusLabel, 'Active', 'success');
            } else if (enabled) {
                this._extensionStatusRow.subtitle = 'Enabled but not active in this GNOME Shell session.';
                setStatusLabel(this._extensionStatusLabel, 'Enabled', 'warning');
            } else {
                this._extensionStatusRow.subtitle = 'Installed but disabled.';
                setStatusLabel(this._extensionStatusLabel, 'Disabled', 'warning');
            }
        } catch (error) {
            this._extensionStatusRow.subtitle = error.message;
            setStatusLabel(this._extensionStatusLabel, 'Missing', 'warning');
        }

        try {
            const active = (await runCommand(['systemctl', '--user', 'is-active', DAEMON_SERVICE])).trim();
            let enabled = 'unknown';
            try {
                enabled = (await runCommand(['systemctl', '--user', 'is-enabled', DAEMON_SERVICE])).trim();
            } catch (_error) {
                enabled = 'not enabled';
            }

            this._serviceStatusRow.subtitle = `Service is ${active}; startup is ${enabled}.`;
            setStatusLabel(this._serviceStatusLabel, active === 'active' ? 'Running' : active, active === 'active' ? 'success' : 'warning');
        } catch (error) {
            this._serviceStatusRow.subtitle = error.message;
            setStatusLabel(this._serviceStatusLabel, 'Not Running', 'warning');
        }

        try {
            const output = await runCli(['daemon-status']);
            const state = output.match(/^state:\s*(.+)$/m)?.[1] ?? 'responding';
            this._daemonStatusRow.subtitle = output.split('\n').slice(0, 2).join(' - ');
            setStatusLabel(this._daemonStatusLabel, state, 'success');
        } catch (error) {
            this._daemonStatusRow.subtitle = error.message;
            setStatusLabel(this._daemonStatusLabel, 'Unavailable', 'warning');
        } finally {
            this._refreshIntegrationButton.sensitive = true;
        }
    }

    _saveExtensionPreferences() {
        if (!this._extensionSettings)
            return;

        const shortcut = this._selectedExtensionShortcut();
        if (!shortcut)
            throw new Error('Enter a custom record / stop shortcut or choose the default shortcut.');

        this._extensionSettings.set_strv(TOGGLE_RECORDING_KEY, [shortcut]);
        this._extensionSettings.set_boolean(CHECK_UPDATES_KEY, this._updateChecksRow.selected === 1);
    }

    async _refreshChecks() {
        this._refreshChecksButton.sensitive = false;
        this._clearRows(this._setupGroup, this._setupRows);
        this._addInfoRow(this._setupGroup, this._setupRows, 'Checking setup', 'Running Chirper diagnostics.');

        try {
            this._checks = await runCliJson(['onboarding-check', '--json']);
            this._renderChecks();
        } catch (error) {
            this._clearRows(this._setupGroup, this._setupRows);
            this._addInfoRow(this._setupGroup, this._setupRows, 'Setup checks unavailable', error.message);
        } finally {
            this._refreshChecksButton.sensitive = true;
            this._syncNavigation();
        }
    }

    _renderChecks() {
        this._clearRows(this._setupGroup, this._setupRows);
        const checks = this._checks;
        const commandRows = [
            ['pw_record', 'PipeWire recorder', 'Records microphone audio for the test.'],
            ['whisper', 'whisper.cpp', 'Transcribes the recorded audio.'],
            ['ollama', 'Ollama', 'Runs local formatting models.'],
            ['codex', 'Codex CLI', 'Optional cloud formatter comparison.'],
        ];

        for (const [key, title, subtitle] of commandRows) {
            const info = commandInfo(checks, key);
            const row = new Adw.ActionRow({
                title,
                subtitle: `${subtitle} ${info.command ? `Command: ${info.command}.` : ''}`,
            });
            row.add_suffix(statusLabel(info.available ? 'Installed' : 'Missing', info.available ? 'success' : 'warning'));
            this._setupGroup.add(row);
            this._setupRows.push(row);
        }

        for (const model of WHISPER_MODELS) {
            const info = whisperInfo(checks, model);
            const row = new Adw.ActionRow({
                title: `Whisper ${whisperLabel(model)}`,
                subtitle: info?.installed ? String(info.path) : 'Required for the transcription comparison.',
            });
            if (info?.installed) {
                row.add_suffix(statusLabel('Installed', 'success'));
            } else {
                const button = makeButton('Download', () => this._downloadWhisperModel(model, row));
                row.add_suffix(button);
                row.activatable_widget = button;
            }
            this._setupGroup.add(row);
            this._setupRows.push(row);
        }

        for (const model of OLLAMA_MODELS) {
            const info = ollamaInfo(checks, model);
            const row = new Adw.ActionRow({
                title: `Ollama ${model}`,
                subtitle: modelNote(model) ?? modelLabel(model),
            });
            if (info?.installed) {
                row.add_suffix(statusLabel('Installed', 'success'));
            } else {
                const button = makeButton('Pull', () => this._pullOllamaModel(model, row));
                button.sensitive = Boolean(commandInfo(checks, 'ollama').available);
                row.add_suffix(button);
                row.activatable_widget = button;
            }
            this._setupGroup.add(row);
            this._setupRows.push(row);
        }

        const codexAvailable = Boolean(commandInfo(checks, 'codex').available);
        this._includeCodexRow.sensitive = codexAvailable;
        if (!codexAvailable) {
            this._includeCodexRow.active = false;
            this._includeCodexRow.subtitle = 'Codex CLI is not installed or is not available on PATH.';
        } else {
            this._includeCodexRow.subtitle = `Uses ${CODEX_MODEL} at ${CODEX_EFFORT} effort if enabled.`;
        }
    }

    async _downloadWhisperModel(model, row) {
        row.subtitle = 'Downloading. This can take a while.';

        try {
            await runCli(['model-download', model]);
            row.subtitle = 'Installed';
            await this._refreshChecks();
        } catch (error) {
            row.subtitle = error.message;
        }
    }

    async _pullOllamaModel(model, row) {
        const command = commandInfo(this._checks, 'ollama').command || 'ollama';
        row.subtitle = 'Pulling model. This can take a while.';

        try {
            await runCommand([command, 'pull', model]);
            row.subtitle = 'Installed';
            await this._refreshChecks();
        } catch (error) {
            row.subtitle = error.message;
        }
    }

    async _toggleRecording() {
        if (this._recording) {
            await this._stopRecording();
            return;
        }

        await this._startRecording();
    }

    async _startRecording() {
        this._recordButton.sensitive = false;
        this._recordStatusRow.subtitle = 'Starting recorder';

        try {
            const statePath = runtimeStatePath();
            const data = await runCliJson(['record-start', '--json', '--state', statePath]);
            this._recording = true;
            this._recordingPath = data.path;
            this._recordButton.label = 'Stop';
            this._recordButton.remove_css_class('suggested-action');
            this._recordButton.add_css_class('destructive-action');
            this._recordStatusRow.subtitle = 'Recording. Read the paragraph, then stop.';
        } catch (error) {
            this._recordStatusRow.subtitle = error.message;
        } finally {
            this._recordButton.sensitive = true;
        }
    }

    async _stopRecording() {
        this._recordButton.sensitive = false;
        this._recordStatusRow.subtitle = 'Stopping recorder';

        try {
            const statePath = runtimeStatePath();
            const data = await runCliJson(['record-stop', '--json', '--state', statePath]);
            this._recording = false;
            this._recordingPath = data.path;
            this._recordButton.label = 'Record Again';
            this._recordButton.remove_css_class('destructive-action');
            this._recordButton.add_css_class('suggested-action');
            this._recordStatusRow.subtitle = `Recorded ${data.path}`;
            await this._runWhisperTests(data.path);
        } catch (error) {
            this._recordStatusRow.subtitle = error.message;
        } finally {
            this._recording = false;
            this._recordButton.sensitive = true;
            this._syncNavigation();
        }
    }

    async _runWhisperTests(audioPath) {
        this._clearRows(this._whisperGroup, this._whisperRows);
        this._clearRows(this._formatGroup, this._formatRows);
        this._whisperResults.clear();
        this._formatResults.clear();
        this._selectedWhisper = null;
        this._selectedFormatter = null;
        this._formatStatusRow.subtitle = 'Waiting for a selected transcript.';
        this._retryPromptGroup.visible = false;
        this._addInfoRow(this._whisperGroup, this._whisperRows, 'Transcribing', 'Running Whisper models one at a time.');

        for (const model of WHISPER_MODELS) {
            const info = whisperInfo(this._checks, model);
            if (!info?.installed) {
                this._addWhisperResult(model, null, 'Model is not installed.');
                continue;
            }

            try {
                const data = await runCliJson([
                    'transcribe-file',
                    '--json',
                    '--profile',
                    'balanced',
                    audioPath,
                    String(info.path),
                ]);
                this._addWhisperResult(
                    model,
                    String(data.text ?? '').trim(),
                    null,
                    data.elapsed_ms,
                    data.metrics
                );
            } catch (error) {
                this._addWhisperResult(model, null, error.message);
            }
        }

        this._syncNavigation();
    }

    _addWhisperResult(model, transcript, error, elapsedMs = null, metrics = null) {
        this._whisperResults.set(model, { transcript, error, elapsedMs, metrics });
        this._renderWhisperResults();
    }

    _renderWhisperResults() {
        this._clearRows(this._whisperGroup, this._whisperRows);
        const resultRow = new Gtk.Box({
            orientation: Gtk.Orientation.HORIZONTAL,
            spacing: 12,
            homogeneous: true,
        });
        const requiresSelection = this._selectedWhisper === null
            && [...this._whisperResults.values()].some(result => !result.error);

        if (requiresSelection) {
            this._addSelectionRequiredRow(
                this._whisperGroup,
                this._whisperRows,
                'Choose a transcript to continue',
                'Nothing is selected by default. Compare the results, then pick the transcript you want Chirper to use.'
            );
        }

        for (const [name, result] of this._whisperResults.entries()) {
            const selected = this._selectedWhisper === name;
            const card = new Gtk.Box({
                orientation: Gtk.Orientation.VERTICAL,
                spacing: 10,
                margin_top: 8,
                margin_bottom: 8,
                margin_start: 8,
                margin_end: 8,
            });
            card.add_css_class('comparison-card');
            if (selected)
                card.add_css_class('comparison-card-selected');
            card.hexpand = true;

            const header = new Gtk.Box({
                orientation: Gtk.Orientation.HORIZONTAL,
                spacing: 12,
            });
            const titleBox = new Gtk.Box({
                orientation: Gtk.Orientation.VERTICAL,
                spacing: 2,
                hexpand: true,
            });
            const title = new Gtk.Label({
                label: `Whisper ${whisperLabel(name)}`,
                xalign: 0,
            });
            title.add_css_class('heading');
            const subtitle = new Gtk.Label({
                label: result.error ? result.error : metricsSummary(result.elapsedMs, result.metrics),
                xalign: 0,
                wrap: true,
            });
            subtitle.add_css_class('dim-label');
            titleBox.append(title);
            titleBox.append(subtitle);
            const button = makeButton(selected ? 'Selected' : 'Choose', () => this._selectWhisper(name));
            button.sensitive = !result.error;
            if (selected)
                button.add_css_class('suggested-action');
            header.append(titleBox);
            header.append(button);
            card.append(header);

            const transcriptLabel = new Gtk.Label({
                label: `Transcript from ${name}`,
                xalign: 0,
            });
            transcriptLabel.add_css_class('caption-heading');
            card.append(transcriptLabel);
            const outputView = textView(result.error ? result.error : result.transcript, {
                minHeight: 420,
                maxHeight: 560,
                monospace: true,
            });
            card.append(outputView);

            resultRow.append(card);
        }

        this._whisperGroup.add(resultRow);
        this._whisperRows.push(resultRow);
    }

    _selectWhisper(model) {
        this._selectedWhisper = model;
        this._recommendationSaved = false;
        this._renderWhisperResults();
        this._syncNavigation();
        this._formatStatusRow.subtitle = 'Ready to run formatting test.';
    }

    async _runFormattingTests() {
        const whisperResult = this._whisperResults.get(this._selectedWhisper);
        if (!whisperResult?.transcript) {
            this._formatStatusRow.subtitle = 'Choose a transcript first.';
            return;
        }

        this._clearRows(this._formatGroup, this._formatRows);
        this._formatResults.clear();
        this._selectedFormatter = null;
        this._formatButton.sensitive = false;
        this._formatStatusRow.subtitle = 'Running local formatting models.';
        this._retryPromptGroup.visible = false;

        for (const model of OLLAMA_MODELS) {
            if (!ollamaInfo(this._checks, model)?.installed) {
                this._addFormatResult(formatterIdForOllama(model), modelChoiceLabel(model), null, 'Model is not installed.');
                continue;
            }

            try {
                const data = await runCliJson([
                    'format-compare',
                    '--json',
                    '--model',
                    model,
                    '--prompt-input',
                    'raw',
                    '--custom-prompt',
                    EXTENSION_FORMATTING_PROMPT,
                    whisperResult.transcript,
                ]);
                const result = data.results?.[0] ?? {};
                this._addFormatResult(
                    formatterIdForOllama(model),
                    modelChoiceLabel(model),
                    result.output,
                    result.error,
                    result.elapsed_ms,
                    result.metrics
                );
            } catch (error) {
                this._addFormatResult(formatterIdForOllama(model), modelChoiceLabel(model), null, error.message);
            }
        }

        if (this._includeCodexRow.active) {
            this._formatStatusRow.subtitle = 'Running Codex formatting.';
            try {
                await runCli(['codex-use', CODEX_MODEL, '--effort', CODEX_EFFORT, '--no-enable']);
                const data = await runCliJson([
                    'format-compare',
                    '--json',
                    '--no-ollama',
                    '--codex',
                    '--prompt-input',
                    'raw',
                    '--custom-prompt',
                    EXTENSION_FORMATTING_PROMPT,
                    whisperResult.transcript,
                ]);
                const result = data.results?.[0] ?? {};
                this._addFormatResult(
                    'codex',
                    `Codex (${CODEX_MODEL}, ${CODEX_EFFORT})`,
                    result.output,
                    result.error,
                    result.elapsed_ms,
                    result.metrics
                );
            } catch (error) {
                this._addFormatResult('codex', 'Codex', null, error.message);
            }
        }

        this._formatStatusRow.subtitle = 'Formatting test finished. Choose an output to continue.';
        this._formatButton.sensitive = true;
        this._retryPromptGroup.visible = this._formatResults.size > 0;
        this._syncNavigation();
    }

    _addFormatResult(id, title, output, error, elapsedMs = null, metrics = null) {
        this._formatResults.set(id, { title, output, error, elapsedMs, metrics });
        this._renderFormatResults();
    }

    _renderFormatResults() {
        this._clearRows(this._formatGroup, this._formatRows);
        const resultRow = new Gtk.Box({
            orientation: Gtk.Orientation.HORIZONTAL,
            spacing: 12,
            homogeneous: true,
        });
        const requiresSelection = this._selectedFormatter === null
            && [...this._formatResults.values()].some(result => !result.error);

        if (requiresSelection) {
            this._addSelectionRequiredRow(
                this._formatGroup,
                this._formatRows,
                'Choose a formatted output to continue',
                'Nothing is selected by default. Pick the exact output you would want Chirper to paste.'
            );
        }

        for (const [id, result] of this._formatResults.entries()) {
            const selected = this._selectedFormatter === id;
            const card = new Gtk.Box({
                orientation: Gtk.Orientation.VERTICAL,
                spacing: 10,
                margin_top: 8,
                margin_bottom: 8,
                margin_start: 8,
                margin_end: 8,
            });
            card.add_css_class('comparison-card');
            if (selected)
                card.add_css_class('comparison-card-selected');
            card.hexpand = true;

            const header = new Gtk.Box({
                orientation: Gtk.Orientation.HORIZONTAL,
                spacing: 12,
            });
            const titleBox = new Gtk.Box({
                orientation: Gtk.Orientation.VERTICAL,
                spacing: 2,
                hexpand: true,
            });
            const title = new Gtk.Label({
                label: result.title,
                xalign: 0,
                wrap: true,
            });
            title.add_css_class('heading');
            const subtitle = new Gtk.Label({
                label: result.error ? result.error : metricsSummary(result.elapsedMs, result.metrics),
                xalign: 0,
                wrap: true,
            });
            subtitle.add_css_class('dim-label');
            titleBox.append(title);
            const formatter = parseFormatterId(id);
            const note = formatter.type === 'ollama' ? modelNote(formatter.model) : null;
            if (note) {
                const noteLabel = new Gtk.Label({
                    label: note,
                    xalign: 0,
                    wrap: true,
                });
                noteLabel.add_css_class('caption');
                titleBox.append(noteLabel);
            }
            titleBox.append(subtitle);
            const button = makeButton(selected ? 'Selected' : 'Choose', () => this._selectFormatter(id));
            button.sensitive = !result.error;
            if (selected)
                button.add_css_class('suggested-action');
            header.append(titleBox);
            header.append(button);
            card.append(header);

            const outputLabel = new Gtk.Label({
                label: `Formatted output from ${result.title}`,
                xalign: 0,
                wrap: true,
            });
            outputLabel.add_css_class('caption-heading');
            card.append(outputLabel);
            const outputView = textView(result.error ? result.error : result.output, {
                minHeight: 420,
                maxHeight: 560,
                monospace: true,
            });
            card.append(outputView);

            resultRow.append(card);
        }

        this._formatGroup.add(resultRow);
        this._formatRows.push(resultRow);
    }

    _selectFormatter(id) {
        this._selectedFormatter = id;
        this._recommendationSaved = false;
        this._renderFormatResults();
        this._syncNavigation();
    }

    async _toggleFormatRetryRecording() {
        if (this._formatRetryRecording) {
            await this._stopFormatRetryRecording();
            return;
        }

        await this._startFormatRetryRecording();
    }

    _setFormatRetryButtonRecording(recording) {
        this._retryPromptButton.label = recording ? 'Stop' : 'Record New Prompt';
        this._retryPromptButton.remove_css_class(recording ? 'suggested-action' : 'destructive-action');
        this._retryPromptButton.add_css_class(recording ? 'destructive-action' : 'suggested-action');
    }

    async _startFormatRetryRecording() {
        if (!this._selectedWhisper) {
            this._retryPromptRow.subtitle = 'Choose a Whisper transcript before recording another formatter prompt.';
            return;
        }

        this._retryPromptButton.sensitive = false;
        this._formatButton.sensitive = false;
        this._retryPromptRow.subtitle = 'Starting recorder.';

        try {
            const statePath = runtimeStatePath('onboarding-format-record-state');
            const data = await runCliJson(['record-start', '--json', '--state', statePath]);
            this._formatRetryRecording = true;
            this._formatRetryRecordingPath = data.path;
            this._selectedFormatter = null;
            this._recommendationSaved = false;
            this._setFormatRetryButtonRecording(true);
            this._renderFormatResults();
            this._formatStatusRow.subtitle = 'Recording a new formatter prompt. Stop recording to transcribe and rerun the comparison.';
            this._retryPromptRow.subtitle = `Recording. Chirper will use Whisper ${whisperLabel(this._selectedWhisper)} and rerun the formatter models when you stop.`;
        } catch (error) {
            this._retryPromptRow.subtitle = error.message;
            this._formatStatusRow.subtitle = error.message;
        } finally {
            this._retryPromptButton.sensitive = true;
            this._syncNavigation();
        }
    }

    async _stopFormatRetryRecording() {
        this._retryPromptButton.sensitive = false;
        this._formatButton.sensitive = false;
        this._retryPromptRow.subtitle = 'Stopping recorder.';

        try {
            const statePath = runtimeStatePath('onboarding-format-record-state');
            const data = await runCliJson(['record-stop', '--json', '--state', statePath]);
            this._formatRetryRecording = false;
            this._formatRetryRecordingPath = data.path;
            this._setFormatRetryButtonRecording(false);
            this._retryPromptRow.subtitle = `Recorded ${data.path}. Transcribing with Whisper ${whisperLabel(this._selectedWhisper)}.`;

            await this._transcribeSelectedWhisper(data.path);
            this._retryPromptRow.subtitle = `Transcribed with Whisper ${whisperLabel(this._selectedWhisper)}. Rerunning formatter models.`;
            await this._runFormattingTests();
            this._retryPromptRow.subtitle = 'Try a numbered list, a note below a list, a URL or email address, a budget, and names like systemd, PostgreSQL, FFmpeg, or GNOME.';
        } catch (error) {
            this._formatRetryRecording = false;
            this._setFormatRetryButtonRecording(false);
            this._retryPromptRow.subtitle = error.message;
            this._formatStatusRow.subtitle = error.message;
        } finally {
            this._formatRetryRecording = false;
            this._setFormatRetryButtonRecording(false);
            this._retryPromptButton.sensitive = true;
            this._formatButton.sensitive = true;
            this._syncNavigation();
        }
    }

    async _transcribeSelectedWhisper(audioPath) {
        const model = this._selectedWhisper;
        const info = whisperInfo(this._checks, model);
        if (!model || !info?.installed)
            throw new Error('Selected Whisper model is not installed.');

        const data = await runCliJson([
            'transcribe-file',
            '--json',
            '--profile',
            'balanced',
            audioPath,
            String(info.path),
        ]);
        const transcript = String(data.text ?? '').trim();
        if (!transcript)
            throw new Error(`Whisper ${whisperLabel(model)} returned an empty transcript.`);

        this._whisperResults.set(model, {
            transcript,
            error: null,
            elapsedMs: data.elapsed_ms,
            metrics: data.metrics,
        });
        this._renderWhisperResults();
    }

    _refreshRecommendation() {
        this._clearRows(this._recommendationGroup, this._saveRows);
        const formatter = parseFormatterId(this._selectedFormatter);
        const whisperRow = new Adw.ActionRow({
            title: 'Whisper Model',
            subtitle: this._selectedWhisper ? whisperLabel(this._selectedWhisper) : 'Not selected',
        });
        this._recommendationGroup.add(whisperRow);
        this._saveRows.push(whisperRow);

        const formatterRow = new Adw.ActionRow({
            title: 'Formatter',
            subtitle: formatter.type === 'codex'
                ? `${CODEX_MODEL} at ${CODEX_EFFORT} effort with a local fallback`
                : modelChoiceLabel(formatter.model),
        });
        this._recommendationGroup.add(formatterRow);
        this._saveRows.push(formatterRow);

        this._fallbackGroup.visible = formatter.type === 'codex';
        if (formatter.type === 'codex') {
            const fallback = this._recommendedFallbackModel();
            const index = Math.max(0, OLLAMA_MODELS.indexOf(fallback));
            this._updatingRecommendationControls = true;
            this._fallbackRow.selected = index;
            this._updatingRecommendationControls = false;
        }
    }

    _recommendedFallbackModel() {
        const firstSuccessfulLocal = [...this._formatResults.entries()]
            .find(([id, result]) => id.startsWith('ollama:') && !result.error);

        if (firstSuccessfulLocal)
            return parseFormatterId(firstSuccessfulLocal[0]).model;

        if (ollamaInfo(this._checks, 'granite4.1:8b')?.installed)
            return 'granite4.1:8b';

        return 'granite4.1:3b';
    }

    _selectedFallbackModel() {
        return OLLAMA_MODELS[this._fallbackRow.selected] ?? this._recommendedFallbackModel();
    }

    async _saveRecommendation() {
        const formatter = parseFormatterId(this._selectedFormatter);
        this._saveButton.sensitive = false;
        this._saveStatusRow.subtitle = 'Saving configuration';

        try {
            await runCli(['model-use', this._selectedWhisper]);

            if (formatter.type === 'codex') {
                const fallback = this._selectedFallbackModel();
                await runCli(['ollama-use', fallback, '--no-enable']);
                await runCli(['codex-use', CODEX_MODEL, '--effort', CODEX_EFFORT, '--enable']);
            } else if (formatter.type === 'ollama') {
                if (formatter.model === 'granite4.1:3b') {
                    await runCli(['ai-format-use', 'low']);
                } else if (formatter.model === 'granite4.1:8b') {
                    await runCli(['ai-format-use', 'medium']);
                } else {
                    await runCli(['ollama-use', formatter.model, '--enable']);
                }
            }

            this._saveExtensionPreferences();

            if (this._removeUnusedRow.active)
                await this._removeUnusedModels();

            this._recommendationSaved = true;
            this._saveStatusRow.subtitle = 'Saved configuration and extension preferences';
            this._syncNavigation();
        } catch (error) {
            this._recommendationSaved = false;
            this._saveStatusRow.subtitle = error.message;
        } finally {
            this._saveButton.sensitive = true;
        }
    }

    async _removeUnusedModels() {
        const formatter = parseFormatterId(this._selectedFormatter);
        const keepOllama = formatter.type === 'codex'
            ? this._selectedFallbackModel()
            : formatter.model;

        for (const model of WHISPER_MODELS) {
            if (model === this._selectedWhisper)
                continue;

            const info = whisperInfo(this._checks, model);
            if (info?.installed && info.path) {
                try {
                    GLib.unlink(String(info.path));
                } catch (error) {
                    console.debug(`failed to remove Whisper model ${model}: ${error.message}`);
                }
            }
        }

        const ollamaCommand = commandInfo(this._checks, 'ollama').command || 'ollama';
        for (const model of OLLAMA_MODELS) {
            if (model === keepOllama)
                continue;

            if (ollamaInfo(this._checks, model)?.installed) {
                try {
                    await runCommand([ollamaCommand, 'rm', model]);
                } catch (error) {
                    console.debug(`failed to remove Ollama model ${model}: ${error.message}`);
                }
            }
        }
    }

    _openConfigFolder() {
        GLib.mkdir_with_parents(configDir(), 0o755);
        runDetached(['xdg-open', configDir()]);
    }
});

const app = new Adw.Application({
    application_id: 'local.chirper.Onboarding',
    flags: Gio.ApplicationFlags.FLAGS_NONE,
});

app.connect('activate', application => {
    installCss();

    if (!window)
        window = new OnboardingWindow(application);

    window.present();
});

app.run(ARGV);
