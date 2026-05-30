import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import St from 'gi://St';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

Gio._promisify(Gio.SocketClient.prototype, 'connect_async', 'connect_finish');
Gio._promisify(Gio.OutputStream.prototype, 'write_bytes_async');
Gio._promisify(
    Gio.Subprocess.prototype,
    'communicate_utf8_async',
    'communicate_utf8_finish'
);
Gio._promisify(Gio.Subprocess.prototype, 'wait_check_async', 'wait_check_finish');
Gio._promisify(
    Gio.DataInputStream.prototype,
    'read_line_async',
    'read_line_finish_utf8'
);

const POLL_SECONDS = 1;
const STATUS_REQUEST_TIMEOUT_SECONDS = 2;
const COMMAND_REQUEST_TIMEOUT_SECONDS = 15;
const STOP_RECORDING_TIMEOUT_SECONDS = 30 * 60;
const MENU_ITEM_LABEL_MAX = 34;
const MENU_STATUS_LABEL_MAX = 24;
const OVERLAY_WIDTH = 360;
const OVERLAY_HEIGHT = 82;
const DAEMON_SERVICE = 'chirper-daemon.service';
const TOGGLE_RECORDING_KEY = 'toggle-recording';
const PASTE_AFTER_STOP_KEY = 'paste-after-stop';
const COMMON_WHISPER_DOWNLOADS = [
    'base',
    'small.en',
    'small',
    'medium',
    'large-v3-turbo',
    'large-v3-turbo-q5_0',
];

function daemonSocketPath() {
    const runtimeDir = GLib.getenv('XDG_RUNTIME_DIR');

    if (runtimeDir)
        return GLib.build_filenamev([runtimeDir, 'chirper', 'daemon.sock']);

    return '/tmp/chirper/daemon.sock';
}

async function sendDaemonRequest(
    command,
    cancellable = null,
    timeoutSeconds = STATUS_REQUEST_TIMEOUT_SECONDS,
    extra = {}
) {
    const client = new Gio.SocketClient({timeout: timeoutSeconds});
    const address = new Gio.UnixSocketAddress({path: daemonSocketPath()});
    const connection = await client.connect_async(address, cancellable);

    try {
        const payload = `${JSON.stringify({command, ...extra})}\n`;
        const output = connection.get_output_stream();
        await output.write_bytes_async(
            new GLib.Bytes(payload),
            GLib.PRIORITY_DEFAULT,
            cancellable
        );
        connection.get_socket().shutdown(false, true);

        const input = new Gio.DataInputStream({
            base_stream: connection.get_input_stream(),
            close_base_stream: false,
        });
        const [line] = await input.read_line_async(GLib.PRIORITY_DEFAULT, cancellable);

        if (line === null)
            throw new Error('daemon returned an empty response');

        const text = typeof line === 'string' ? line : new TextDecoder().decode(line);
        return JSON.parse(text);
    } finally {
        try {
            connection.close(null);
        } catch (error) {
            console.debug(`Chirper: failed to close daemon socket: ${error}`);
        }
    }
}

async function controlDaemonService(action, cancellable = null) {
    const process = Gio.Subprocess.new(
        ['systemctl', '--user', action, DAEMON_SERVICE],
        Gio.SubprocessFlags.STDOUT_SILENCE | Gio.SubprocessFlags.STDERR_SILENCE
    );

    await process.wait_check_async(cancellable);
}

async function runCommand(argv, cancellable = null) {
    const process = Gio.Subprocess.new(
        argv,
        Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE
    );
    const [stdout, stderr] = await process.communicate_utf8_async(null, cancellable);

    if (!process.get_successful())
        throw new Error(stderr.trim() || `${argv[0]} failed`);

    return stdout;
}

function loadJsonFile(path) {
    const file = Gio.File.new_for_path(path);

    try {
        const [, contents] = file.load_contents(null);
        return JSON.parse(new TextDecoder().decode(contents));
    } catch (error) {
        console.debug(`Chirper: failed to load ${path}: ${error.message}`);
        return {};
    }
}

function sleep(milliseconds) {
    return new Promise(resolve => {
        GLib.timeout_add(GLib.PRIORITY_DEFAULT, milliseconds, () => {
            resolve();
            return GLib.SOURCE_REMOVE;
        });
    });
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
        .replace(/\bsuper\b/i, 'Super');
}

function shortLabel(value, maxLength = 34) {
    if (!value)
        return '';

    if (value.length <= maxLength)
        return value;

    return `${value.slice(0, maxLength - 1)}…`;
}

export default class ChirperExtension extends Extension {
    enable() {
        this._enabled = true;
        this._cancellable = new Gio.Cancellable();
        this._settings = this.getSettings();
        this._runtime = loadJsonFile(GLib.build_filenamev([this.path, 'runtime.json']));
        this._pendingStatus = false;
        this._lastState = 'unknown';
        this._lastFocusedWindow = null;
        this._virtualKeyboard = null;
        this._selectedAudioTarget = null;
        this._activeAudioLabel = null;

        this._rememberFocusedWindow();
        this._focusWindowId = global.display.connect('notify::focus-window', () => {
            this._rememberFocusedWindow();
        });

        this._buildPanelIndicator();
        this._buildOverlay();
        this._registerKeybinding();
        this._refreshStatus();

        this._pollSourceId = GLib.timeout_add_seconds(
            GLib.PRIORITY_DEFAULT,
            POLL_SECONDS,
            () => {
                this._refreshStatus();
                return GLib.SOURCE_CONTINUE;
            }
        );
    }

    disable() {
        this._enabled = false;
        this._cancellable?.cancel();
        this._cancellable = null;

        if (this._pollSourceId) {
            GLib.source_remove(this._pollSourceId);
            this._pollSourceId = 0;
        }

        if (this._pulseSourceId) {
            GLib.source_remove(this._pulseSourceId);
            this._pulseSourceId = 0;
        }

        if (this._settingsChangedId) {
            this._settings.disconnect(this._settingsChangedId);
            this._settingsChangedId = 0;
        }

        if (this._focusWindowId) {
            global.display.disconnect(this._focusWindowId);
            this._focusWindowId = 0;
        }

        if (this._menuOpenId) {
            this._indicator.menu.disconnect(this._menuOpenId);
            this._menuOpenId = 0;
        }

        if (this._monitorsChangedId) {
            Main.layoutManager.disconnect(this._monitorsChangedId);
            this._monitorsChangedId = 0;
        }

        Main.wm.removeKeybinding(TOGGLE_RECORDING_KEY);

        this._overlay?.destroy();
        this._overlay = null;
        this._overlayIcon = null;
        this._overlayLabel = null;

        this._indicator?.destroy();
        this._indicator = null;
        this._icon = null;
        this._statusItem = null;
        this._primaryItem = null;
        this._primaryIcon = null;
        this._primaryLabel = null;
        this._primarySubLabel = null;
        this._shortcutLabel = null;
        this._pasteSwitch = null;
        this._audioMenu = null;
        this._screenAudioMenu = null;
        this._modelMenu = null;
        this._ollamaMenu = null;
        this._selectedAudioTarget = null;
        this._activeAudioLabel = null;
        this._runtime = null;
        this._settings = null;
    }

    _buildPanelIndicator() {
        this._indicator = new PanelMenu.Button(0.0, this.metadata.name, false);
        this._indicator.menu.box.add_style_class_name('chirper-menu');
        this._icon = new St.Icon({
            icon_name: 'audio-input-microphone-symbolic',
            style_class: 'system-status-icon',
        });
        this._indicator.add_child(this._icon);
        Main.panel.addToStatusArea(this.uuid, this._indicator);
        this._menuOpenId = this._indicator.menu.connect('open-state-changed', (_menu, isOpen) => {
            if (isOpen)
                this._rememberFocusedWindow();
        });

        this._statusItem = new PopupMenu.PopupMenuItem('Chirper: connecting', {
            reactive: false,
        });
        this._indicator.menu.addMenuItem(this._statusItem);
        this._indicator.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        this._primaryItem = new PopupMenu.PopupBaseMenuItem({
            style_class: 'chirper-primary-item',
        });
        this._primaryIcon = new St.Icon({
            icon_name: 'audio-input-microphone-symbolic',
            style_class: 'chirper-primary-icon',
        });
        const labelBox = new St.BoxLayout({
            vertical: true,
            x_expand: true,
        });
        this._primaryLabel = new St.Label({
            text: 'Start Recording',
            style_class: 'chirper-primary-title',
        });
        this._primarySubLabel = new St.Label({
            text: 'Ready',
            style_class: 'chirper-primary-subtitle',
        });
        labelBox.add_child(this._primaryLabel);
        labelBox.add_child(this._primarySubLabel);
        this._shortcutLabel = new St.Label({
            text: '',
            style_class: 'chirper-shortcut',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._primaryItem.add_child(this._primaryIcon);
        this._primaryItem.add_child(labelBox);
        this._primaryItem.add_child(this._shortcutLabel);
        this._primaryItem.connect('activate', () => {
            this._runPrimaryAction();
        });
        this._indicator.menu.addMenuItem(this._primaryItem);

        this._indicator.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        this._pasteSwitch = new PopupMenu.PopupSwitchMenuItem(
            'Paste After Stop',
            this._settings.get_boolean(PASTE_AFTER_STOP_KEY)
        );
        this._pasteSwitch.connect('toggled', () => {
            this._settings.set_boolean(PASTE_AFTER_STOP_KEY, this._pasteSwitch.state);
            this._syncPrimaryAction();
        });
        this._indicator.menu.addMenuItem(this._pasteSwitch);

        this._indicator.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        this._audioMenu = new PopupMenu.PopupSubMenuMenuItem('Input');
        this._indicator.menu.addMenuItem(this._audioMenu);
        this._audioMenu.menu.addMenuItem(this._disabledItem('Loading audio inputs'));
        this._audioMenu.menu.connect('open-state-changed', (_menu, isOpen) => {
            if (isOpen)
                this._refreshAudioMenu();
        });

        this._screenAudioMenu = new PopupMenu.PopupSubMenuMenuItem('Screen Audio Once');
        this._indicator.menu.addMenuItem(this._screenAudioMenu);
        this._screenAudioMenu.menu.addMenuItem(this._disabledItem('Loading outputs'));
        this._screenAudioMenu.menu.connect('open-state-changed', (_menu, isOpen) => {
            if (isOpen)
                this._refreshAudioMenu();
        });

        this._modelMenu = new PopupMenu.PopupSubMenuMenuItem('Whisper Model');
        this._indicator.menu.addMenuItem(this._modelMenu);
        this._modelMenu.menu.addMenuItem(this._disabledItem('Loading models'));
        this._modelMenu.menu.connect('open-state-changed', (_menu, isOpen) => {
            if (isOpen)
                this._refreshModelMenu();
        });

        this._ollamaMenu = new PopupMenu.PopupSubMenuMenuItem('Formatter');
        this._indicator.menu.addMenuItem(this._ollamaMenu);
        this._ollamaMenu.menu.addMenuItem(this._disabledItem('Loading formatter'));
        this._ollamaMenu.menu.connect('open-state-changed', (_menu, isOpen) => {
            if (isOpen)
                this._refreshOllamaMenu();
        });

        this._indicator.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        const preferencesItem = new PopupMenu.PopupMenuItem('Open Settings Window');
        preferencesItem.connect('activate', () => {
            this._openPreferences();
        });
        this._indicator.menu.addMenuItem(preferencesItem);

        const refreshItem = new PopupMenu.PopupMenuItem('Refresh Status');
        refreshItem.connect('activate', () => {
            this._refreshStatus(true);
        });
        this._indicator.menu.addMenuItem(refreshItem);

        const restartDaemonItem = new PopupMenu.PopupMenuItem('Restart Daemon');
        restartDaemonItem.connect('activate', () => {
            this._controlDaemon('restart');
        });
        this._indicator.menu.addMenuItem(restartDaemonItem);

        const configItem = new PopupMenu.PopupMenuItem('Open Config Folder');
        configItem.connect('activate', () => {
            this._openConfigFolder();
        });
        this._indicator.menu.addMenuItem(configItem);

        this._syncShortcutLabel();
        this._syncPrimaryAction();
        this._refreshAudioMenu();
        this._refreshModelMenu();
        this._refreshOllamaMenu();
    }

    _buildOverlay() {
        this._overlay = new St.BoxLayout({
            style_class: 'chirper-recording-overlay',
            vertical: false,
            visible: false,
            reactive: false,
            opacity: 255,
            x_align: Clutter.ActorAlign.CENTER,
            y_align: Clutter.ActorAlign.CENTER,
        });

        this._overlayIcon = new St.Icon({
            icon_name: 'audio-input-microphone-symbolic',
            style_class: 'chirper-overlay-icon',
        });
        this._overlayIcon.set_pivot_point(0.5, 0.5);
        this._overlayLabel = new St.Label({
            text: 'Recording',
            style_class: 'chirper-overlay-label',
            y_align: Clutter.ActorAlign.CENTER,
        });

        this._overlay.add_child(this._overlayIcon);
        this._overlay.add_child(this._overlayLabel);
        Main.layoutManager.uiGroup.add_child(this._overlay);

        this._positionOverlay();
        this._monitorsChangedId = Main.layoutManager.connect('monitors-changed', () => {
            this._positionOverlay();
        });
    }

    _registerKeybinding() {
        Main.wm.addKeybinding(
            TOGGLE_RECORDING_KEY,
            this._settings,
            Meta.KeyBindingFlags.NONE,
            Shell.ActionMode.NORMAL | Shell.ActionMode.OVERVIEW,
            () => {
                this._runPrimaryAction();
            }
        );
        this._settingsChangedId = this._settings.connect(
            `changed::${TOGGLE_RECORDING_KEY}`,
            () => {
                this._syncShortcutLabel();
            }
        );
    }

    _positionOverlay() {
        if (!this._overlay)
            return;

        const monitor = Main.layoutManager.primaryMonitor;
        const x = monitor.x + Math.floor((monitor.width - OVERLAY_WIDTH) / 2);
        const y = monitor.y + Math.floor(monitor.height * 0.16);

        this._overlay.set_size(OVERLAY_WIDTH, OVERLAY_HEIGHT);
        this._overlay.set_position(x, y);
    }

    async _refreshStatus(force = false) {
        if (this._isProcessingState(this._lastState) && !force)
            return;

        if (this._pendingStatus && !force)
            return;

        this._pendingStatus = true;

        try {
            const response = await sendDaemonRequest(
                'status',
                this._cancellable,
                STATUS_REQUEST_TIMEOUT_SECONDS
            );
            this._applyResponse(response);
        } catch (error) {
            if (this._enabled && !this._cancellable?.is_cancelled())
                this._setDisconnected(error.message);
        } finally {
            this._pendingStatus = false;
        }
    }

    async _runPrimaryAction() {
        if (this._isProcessingState(this._lastState))
            return;

        if (this._lastState === 'recording')
            await this._stopRecordingAndMaybePaste();
        else
            await this._startRecording(this._selectedAudioTarget);
    }

    async _startRecording(audioTarget = null) {
        this._setPrimarySensitive(false);
        const extra = audioTarget ? {audio: audioTarget} : {};

        try {
            const response = await this._sendCommandWithAutoStart(
                'start_recording',
                COMMAND_REQUEST_TIMEOUT_SECONDS,
                extra
            );
            this._applyResponse(response);

            if (!response.ok)
                Main.notify('Chirper', response.message);
        } catch (error) {
            if (this._enabled && !this._cancellable?.is_cancelled()) {
                this._setDisconnected(error.message);
                Main.notify('Chirper', error.message);
            }
        } finally {
            this._setPrimarySensitive(true);
        }
    }

    async _stopRecordingAndMaybePaste() {
        const targetWindow = this._lastFocusedWindow;
        this._setPrimarySensitive(false);
        this._applyLocalState('transcribing', 'Processing dictation');

        try {
            const response = await sendDaemonRequest(
                'stop_recording',
                this._cancellable,
                STOP_RECORDING_TIMEOUT_SECONDS
            );
            this._applyResponse(response);

            if (!response.ok) {
                Main.notify('Chirper', response.message);
                return;
            }

            if (response.copied && this._settings.get_boolean(PASTE_AFTER_STOP_KEY))
                await this._pasteIntoWindow(targetWindow);
        } catch (error) {
            if (this._enabled && !this._cancellable?.is_cancelled()) {
                this._setDisconnected(error.message);
                Main.notify('Chirper', error.message);
            }
        } finally {
            this._setPrimarySensitive(true);
        }
    }

    async _sendCommandWithAutoStart(
        command,
        timeoutSeconds = COMMAND_REQUEST_TIMEOUT_SECONDS,
        extra = {}
    ) {
        try {
            return await sendDaemonRequest(command, this._cancellable, timeoutSeconds, extra);
        } catch (error) {
            await controlDaemonService('start', this._cancellable);
            await sleep(500);
            return await sendDaemonRequest(command, this._cancellable, timeoutSeconds, extra);
        }
    }

    async _controlDaemon(action) {
        try {
            await controlDaemonService(action, this._cancellable);
            await sleep(500);
            await this._refreshStatus(true);
        } catch (error) {
            if (this._enabled && !this._cancellable?.is_cancelled())
                Main.notify('Chirper', `Failed to ${action} daemon: ${error.message}`);
        }
    }

    async _refreshAudioMenu() {
        if (!this._audioMenu || !this._screenAudioMenu)
            return;

        this._audioMenu.menu.removeAll();
        this._screenAudioMenu.menu.removeAll();

        try {
            const output = await this._runCli(['audio-list', '--json']);
            const data = JSON.parse(output);
            const current = data.current ?? {};
            const currentLabel = current.label ?? 'Default microphone';
            const sources = data.sources ?? [];
            const sinks = data.sinks ?? [];

            this._selectedAudioTarget = current.target
                ? {kind: 'input', target: current.target, label: currentLabel}
                : null;
            this._audioMenu.label.text = `Input: ${shortLabel(currentLabel, MENU_STATUS_LABEL_MAX)}`;
            this._audioMenu.menu.addMenuItem(
                this._disabledItem(`Current: ${shortLabel(currentLabel, MENU_ITEM_LABEL_MAX)}`)
            );
            this._audioMenu.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

            const defaultLabel = current.target ? 'Default microphone' : '✓ Default microphone';
            const defaultItem = new PopupMenu.PopupMenuItem(defaultLabel);
            defaultItem.connect('activate', () => {
                this._selectAudioInput('auto');
            });
            this._audioMenu.menu.addMenuItem(defaultItem);

            for (const source of sources) {
                const shortSource = shortLabel(source.label, MENU_ITEM_LABEL_MAX);
                const label = source.selected ? `✓ ${shortSource}` : shortSource;
                const item = new PopupMenu.PopupMenuItem(label);
                item.connect('activate', () => {
                    this._selectAudioInput(source.target);
                });
                this._audioMenu.menu.addMenuItem(item);
            }

            this._screenAudioMenu.label.text = 'Screen Audio Once';
            this._screenAudioMenu.menu.addMenuItem(this._disabledItem('One recording only'));
            this._screenAudioMenu.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

            if (sinks.length === 0) {
                this._screenAudioMenu.menu.addMenuItem(this._disabledItem('No outputs found'));
            } else {
                for (const sink of sinks) {
                    const item = new PopupMenu.PopupMenuItem(
                        shortLabel(sink.label, MENU_ITEM_LABEL_MAX)
                    );
                    item.connect('activate', () => {
                        this._startRecording({
                            kind: 'screen_audio',
                            target: sink.target,
                            label: `Screen audio: ${sink.label}`,
                        });
                    });
                    this._screenAudioMenu.menu.addMenuItem(item);
                }
            }
        } catch (error) {
            this._audioMenu.label.text = 'Input';
            this._screenAudioMenu.label.text = 'Screen Audio Once';
            this._audioMenu.menu.addMenuItem(this._disabledItem('Audio controls unavailable'));
            this._audioMenu.menu.addMenuItem(
                this._disabledItem(shortLabel(error.message, MENU_ITEM_LABEL_MAX))
            );
            this._screenAudioMenu.menu.addMenuItem(this._disabledItem('Audio controls unavailable'));
        }
    }

    async _selectAudioInput(target) {
        try {
            await this._runCli(['audio-use', target]);
            await this._refreshAudioMenu();
            Main.notify('Chirper', 'Audio input updated');
        } catch (error) {
            Main.notify('Chirper', `Failed to select audio input: ${error.message}`);
        }
    }

    async _refreshModelMenu() {
        if (!this._modelMenu)
            return;

        this._modelMenu.menu.removeAll();

        try {
            const output = await this._runCli(['model-list', '--json']);
            const data = JSON.parse(output);
            const current = data.current?.name ?? 'unset';
            const installed = data.installed ?? [];
            const available = data.available ?? [];
            const installedNames = new Set(installed.map(model => model.name));

            this._modelMenu.label.text = `Whisper Model: ${current}`;
            this._modelMenu.menu.addMenuItem(this._disabledItem(`Current: ${current}`));
            this._modelMenu.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

            if (installed.length === 0) {
                this._modelMenu.menu.addMenuItem(this._disabledItem('No local models found'));
            } else {
                for (const model of installed) {
                    const label = model.name === current ? `✓ ${model.name}` : model.name;
                    const item = new PopupMenu.PopupMenuItem(label);
                    item.connect('activate', () => {
                        this._selectWhisperModel(model.name);
                    });
                    this._modelMenu.menu.addMenuItem(item);
                }
            }

            this._modelMenu.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
            this._modelMenu.menu.addMenuItem(this._disabledItem('Download and Select'));

            for (const name of COMMON_WHISPER_DOWNLOADS) {
                if (!available.some(model => model.name === name) || installedNames.has(name))
                    continue;

                const item = new PopupMenu.PopupMenuItem(name);
                item.connect('activate', () => {
                    this._downloadWhisperModel(name);
                });
                this._modelMenu.menu.addMenuItem(item);
            }
        } catch (error) {
            this._modelMenu.label.text = 'Whisper Model';
            this._modelMenu.menu.addMenuItem(this._disabledItem('Model controls unavailable'));
            this._modelMenu.menu.addMenuItem(this._disabledItem(error.message));
        }
    }

    async _selectWhisperModel(model) {
        try {
            await this._runCli(['model-use', model]);
            Main.notify('Chirper', `Selected Whisper model: ${model}`);
            await this._refreshModelMenu();
        } catch (error) {
            Main.notify('Chirper', `Failed to select model: ${error.message}`);
        }
    }

    async _downloadWhisperModel(model) {
        Main.notify('Chirper', `Downloading Whisper model: ${model}`);

        try {
            await this._runCli(['model-download', model, '--select']);
            Main.notify('Chirper', `Selected Whisper model: ${model}`);
            await this._refreshModelMenu();
        } catch (error) {
            Main.notify('Chirper', `Failed to download model: ${error.message}`);
        }
    }

    async _refreshOllamaMenu() {
        if (!this._ollamaMenu)
            return;

        this._ollamaMenu.menu.removeAll();

        try {
            const output = await this._runCli(['ollama-list', '--json']);
            const data = JSON.parse(output);
            const formatter = data.formatter ?? 'rules';
            const current = data.current?.model ?? 'unset';
            const models = data.models ?? [];

            this._ollamaMenu.label.text = formatter === 'ollama'
                ? `Ollama: ${shortLabel(current, MENU_STATUS_LABEL_MAX)}`
                : `Formatter: ${formatter}`;
            this._ollamaMenu.menu.addMenuItem(
                this._disabledItem(`Current: ${formatter === 'ollama' ? current : formatter}`)
            );
            this._ollamaMenu.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

            const rulesItem = new PopupMenu.PopupMenuItem(
                formatter === 'rules' ? '✓ Rules only' : 'Rules only'
            );
            rulesItem.connect('activate', () => {
                this._selectFormatter('rules');
            });
            this._ollamaMenu.menu.addMenuItem(rulesItem);

            const noneItem = new PopupMenu.PopupMenuItem(
                formatter === 'none' ? '✓ No formatter' : 'No formatter'
            );
            noneItem.connect('activate', () => {
                this._selectFormatter('none');
            });
            this._ollamaMenu.menu.addMenuItem(noneItem);

            const codexItem = new PopupMenu.PopupMenuItem(
                formatter === 'codex' ? '✓ Codex CLI' : 'Codex CLI'
            );
            codexItem.connect('activate', () => {
                this._selectFormatter('codex');
            });
            this._ollamaMenu.menu.addMenuItem(codexItem);

            this._ollamaMenu.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

            if (!data.available) {
                this._ollamaMenu.menu.addMenuItem(this._disabledItem('Ollama unavailable'));
                if (data.error)
                    this._ollamaMenu.menu.addMenuItem(
                        this._disabledItem(shortLabel(data.error, MENU_ITEM_LABEL_MAX))
                    );
                return;
            }

            if (models.length === 0) {
                this._ollamaMenu.menu.addMenuItem(this._disabledItem('No Ollama models found'));
                this._ollamaMenu.menu.addMenuItem(this._disabledItem('Run: ollama pull llama3.2'));
                return;
            }

            this._ollamaMenu.menu.addMenuItem(this._disabledItem('Ollama models'));

            for (const model of models) {
                const active = formatter === 'ollama' && model.selected;
                const label = active ? `✓ ${model.name}` : model.name;
                const item = new PopupMenu.PopupMenuItem(shortLabel(label, MENU_ITEM_LABEL_MAX));
                item.connect('activate', () => {
                    this._selectOllamaModel(model.name);
                });
                this._ollamaMenu.menu.addMenuItem(item);
            }
        } catch (error) {
            this._ollamaMenu.label.text = 'Formatter';
            this._ollamaMenu.menu.addMenuItem(this._disabledItem('Formatter controls unavailable'));
            this._ollamaMenu.menu.addMenuItem(
                this._disabledItem(shortLabel(error.message, MENU_ITEM_LABEL_MAX))
            );
        }
    }

    async _selectFormatter(formatter) {
        try {
            await this._runCli(['formatter-use', formatter]);
            Main.notify('Chirper', `Selected formatter: ${formatter}`);
            await this._refreshOllamaMenu();
        } catch (error) {
            Main.notify('Chirper', `Failed to select formatter: ${error.message}`);
        }
    }

    async _selectOllamaModel(model) {
        try {
            await this._runCli(['ollama-use', model]);
            Main.notify('Chirper', `Selected Ollama model: ${model}`);
            await this._refreshOllamaMenu();
        } catch (error) {
            Main.notify('Chirper', `Failed to select Ollama model: ${error.message}`);
        }
    }

    async _runCli(args) {
        const cliPath = this._runtime.cliPath || 'chirper';
        return await runCommand([cliPath, ...args], this._cancellable);
    }

    _disabledItem(label) {
        const item = new PopupMenu.PopupMenuItem(label);
        item.setSensitive(false);
        return item;
    }

    _applyLocalState(state, message) {
        this._lastState = state;
        this._statusItem.label.text = `Chirper: ${message}`;
        this._syncPanelIcon(state);
        this._syncPrimaryAction();
        this._setOverlayState(state);
    }

    _applyResponse(response) {
        const state = response.state ?? 'unknown';
        const message = response.message ?? state;
        this._lastState = state;

        if (response.audio_label)
            this._activeAudioLabel = response.audio_label;
        else if (!this._isProcessingState(state) && state !== 'recording')
            this._activeAudioLabel = null;

        if (!this._statusItem || !this._icon)
            return;

        this._statusItem.label.text = `Chirper: ${message}`;
        this._syncPanelIcon(state);
        this._syncPrimaryAction();
        this._setOverlayState(state);
    }

    _setDisconnected(message) {
        this._lastState = 'disconnected';

        if (!this._statusItem || !this._icon)
            return;

        this._statusItem.label.text = 'Chirper: daemon unavailable';
        this._syncPanelIcon('disconnected');
        this._syncPrimaryAction();
        this._overlay?.hide();
        this._stopOverlayPulse();

        if (message)
            console.debug(`Chirper: ${message}`);
    }

    _syncPanelIcon(state) {
        this._icon.remove_style_class_name('chirper-panel-recording');
        this._icon.remove_style_class_name('chirper-panel-processing');

        if (state === 'recording') {
            this._icon.icon_name = 'media-record-symbolic';
            this._icon.add_style_class_name('chirper-panel-recording');
        } else if (this._isProcessingState(state)) {
            this._icon.icon_name = 'view-refresh-symbolic';
            this._icon.add_style_class_name('chirper-panel-processing');
        } else if (state === 'disconnected') {
            this._icon.icon_name = 'dialog-warning-symbolic';
        } else {
            this._icon.icon_name = 'audio-input-microphone-symbolic';
        }
    }

    _syncPrimaryAction() {
        if (!this._primaryLabel || !this._primarySubLabel || !this._primaryIcon)
            return;

        if (this._lastState === 'recording') {
            this._primaryIcon.icon_name = 'media-playback-stop-symbolic';
            this._primaryLabel.text = 'Stop Recording';
            this._primarySubLabel.text = this._activeAudioLabel
                ? shortLabel(this._activeAudioLabel, 34)
                : this._settings.get_boolean(PASTE_AFTER_STOP_KEY)
                    ? 'Paste after transcription'
                    : 'Copy after transcription';
        } else if (this._isProcessingState(this._lastState)) {
            this._primaryIcon.icon_name = 'view-refresh-symbolic';
            this._primaryLabel.text = 'Processing';
            this._primarySubLabel.text = this._activeAudioLabel
                ? shortLabel(this._activeAudioLabel, 34)
                : 'Transcribing and formatting';
        } else {
            this._primaryIcon.icon_name = 'audio-input-microphone-symbolic';
            this._primaryLabel.text = 'Start Recording';
            this._primarySubLabel.text = this._lastState === 'disconnected'
                ? 'Starts daemon if available'
                : 'Ready';
        }
    }

    _syncShortcutLabel() {
        const shortcuts = this._settings.get_strv(TOGGLE_RECORDING_KEY);
        this._shortcutLabel.text = formatAccelerator(shortcuts[0]);
    }

    _setPrimarySensitive(sensitive) {
        this._primaryItem?.setSensitive(sensitive);
    }

    _setOverlayState(state) {
        if (!this._overlay || !this._overlayLabel || !this._overlayIcon)
            return;

        if (state === 'recording') {
            this._overlayIcon.icon_name = 'media-record-symbolic';
            this._overlayLabel.text = this._overlayText('Recording');
            this._overlay.show();
            this._startOverlayPulse();
            return;
        }

        if (this._isProcessingState(state)) {
            this._overlayIcon.icon_name = 'view-refresh-symbolic';
            this._overlayLabel.text = this._overlayText('Processing');
            this._overlay.show();
            this._startOverlayPulse();
            return;
        }

        this._overlay.hide();
        this._stopOverlayPulse();
    }

    _overlayText(action) {
        if (!this._activeAudioLabel)
            return action;

        return `${action}\n${shortLabel(this._activeAudioLabel, 34)}`;
    }

    _startOverlayPulse() {
        if (this._pulseSourceId)
            return;

        this._pulseOut = false;
        this._pulseSourceId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 650, () => {
            if (!this._overlay?.visible) {
                this._pulseSourceId = 0;
                return GLib.SOURCE_REMOVE;
            }

            const opacity = this._pulseOut ? 255 : 215;
            const scale = this._pulseOut ? 1.0 : 1.14;
            this._overlay.ease({
                opacity,
                duration: 520,
                mode: Clutter.AnimationMode.EASE_IN_OUT_SINE,
            });
            this._overlayIcon.ease({
                scale_x: scale,
                scale_y: scale,
                duration: 520,
                mode: Clutter.AnimationMode.EASE_IN_OUT_SINE,
            });
            this._pulseOut = !this._pulseOut;
            return GLib.SOURCE_CONTINUE;
        });
    }

    _stopOverlayPulse() {
        if (this._pulseSourceId) {
            GLib.source_remove(this._pulseSourceId);
            this._pulseSourceId = 0;
        }

        this._overlay?.ease({
            opacity: 255,
            duration: 120,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
        });

        if (this._overlayIcon) {
            this._overlayIcon.scale_x = 1.0;
            this._overlayIcon.scale_y = 1.0;
        }
    }

    _isProcessingState(state) {
        return state === 'transcribing' || state === 'formatting' || state === 'inserting';
    }

    _rememberFocusedWindow() {
        const window = global.display.focus_window;

        if (window && !window.is_skip_taskbar())
            this._lastFocusedWindow = window;
    }

    async _pasteIntoWindow(window) {
        this._indicator.menu.close();

        if (window)
            Main.activateWindow(window, global.get_current_time());

        await sleep(220);
        this._sendPasteShortcut();
    }

    _sendPasteShortcut() {
        if (!this._virtualKeyboard)
            this._virtualKeyboard = this._createVirtualKeyboard();

        const eventTime = (Clutter.get_current_event_time() || global.get_current_time()) * 1000;
        this._virtualKeyboard.notify_keyval(
            eventTime,
            Clutter.KEY_Control_L,
            Clutter.KeyState.PRESSED
        );
        this._virtualKeyboard.notify_keyval(eventTime, Clutter.KEY_v, Clutter.KeyState.PRESSED);
        this._virtualKeyboard.notify_keyval(eventTime, Clutter.KEY_v, Clutter.KeyState.RELEASED);
        this._virtualKeyboard.notify_keyval(
            eventTime,
            Clutter.KEY_Control_L,
            Clutter.KeyState.RELEASED
        );
    }

    _createVirtualKeyboard() {
        const backend = Clutter.get_default_backend?.() ?? global.stage.context.get_backend();
        return backend
            .get_default_seat()
            .create_virtual_device(Clutter.InputDeviceType.KEYBOARD_DEVICE);
    }

    _openConfigFolder() {
        const configHome = GLib.getenv('XDG_CONFIG_HOME') ??
            GLib.build_filenamev([GLib.get_home_dir(), '.config']);
        const path = GLib.build_filenamev([configHome, 'chirper']);

        try {
            GLib.mkdir_with_parents(path, 0o755);
            Gio.AppInfo.launch_default_for_uri(GLib.filename_to_uri(path, null), null);
        } catch (error) {
            Main.notify('Chirper', `Failed to open config folder: ${error.message}`);
        }
    }

    _openPreferences() {
        const settingsPath = GLib.build_filenamev([this.path, 'settings.js']);

        try {
            Gio.Subprocess.new(
                ['gjs', '-m', settingsPath, this.path],
                Gio.SubprocessFlags.STDOUT_SILENCE | Gio.SubprocessFlags.STDERR_SILENCE
            );
        } catch (error) {
            Main.notify('Chirper', `Failed to open settings: ${error.message}`);
        }
    }
}
