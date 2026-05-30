import {ExtensionPreferences} from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

import {buildPreferencesWindow} from './preferences.js';

export default class ChirperPreferences extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        buildPreferencesWindow(window, this.path, this.getSettings());
    }
}
