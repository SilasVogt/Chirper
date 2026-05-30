import Adw from 'gi://Adw';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import {buildPreferencesWindow, loadExtensionSettings} from './preferences.js';

const extensionPath = ARGV[0] ?? GLib.get_current_dir();

const app = new Adw.Application({
    application_id: 'dev.local.Chirper.Settings',
    flags: Gio.ApplicationFlags.FLAGS_NONE,
});

app.connect('activate', application => {
    const settings = loadExtensionSettings(extensionPath);
    const window = new Adw.PreferencesWindow({
        application,
        title: 'Chirper Settings',
        default_width: 720,
        default_height: 640,
    });

    buildPreferencesWindow(window, extensionPath, settings);
    window.present();
});

app.run([]);
