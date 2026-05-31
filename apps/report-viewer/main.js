import Adw from 'gi://Adw';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Gtk from 'gi://Gtk';
import Pango from 'gi://Pango';

const VIEW_OPTIONS = [
    ['regular', 'Regular Text'],
    ['markdown', 'Markdown Preview'],
];
const SORT_OPTIONS = [
    ['model', 'Model'],
    ['prompt', 'Prompt'],
    ['transcript', 'Transcript'],
    ['runtime', 'Runtime'],
    ['gpu', 'GPU Usage'],
    ['power', 'GPU Power'],
    ['file', 'File'],
];

let window = null;

function defaultReportDir() {
    return GLib.build_filenamev([
        GLib.get_home_dir(),
        'Documents',
        'Chirper Compare Reports',
    ]);
}

function makeStringList(labels) {
    const list = new Gtk.StringList();
    for (const label of labels)
        list.append(label);
    return list;
}

function textFromFile(path) {
    const [ok, bytes] = GLib.file_get_contents(path);
    if (!ok)
        throw new Error(`Could not read ${path}`);

    return new TextDecoder().decode(bytes);
}

function listReportFiles(folderPath) {
    const folder = Gio.File.new_for_path(folderPath);
    const enumerator = folder.enumerate_children(
        'standard::name,standard::type',
        Gio.FileQueryInfoFlags.NONE,
        null
    );
    const paths = [];
    let info = null;

    while ((info = enumerator.next_file(null)) !== null) {
        if (info.get_file_type() !== Gio.FileType.REGULAR)
            continue;

        const name = info.get_name();
        if (name.startsWith('chirper-format-compare-') && name.endsWith('.txt'))
            paths.push(GLib.build_filenamev([folderPath, name]));
    }

    paths.sort();
    return paths;
}

function shortPath(path) {
    return GLib.path_get_basename(path);
}

function compactText(text, limit = 140) {
    const compact = text.replace(/\s+/g, ' ').trim();
    if (compact.length <= limit)
        return compact;

    return `${compact.slice(0, Math.max(0, limit - 1))}...`;
}

function parseDurationToMs(value) {
    const text = value.trim();
    const number = Number.parseFloat(text);
    if (!Number.isFinite(number))
        return null;

    if (text.endsWith('ms'))
        return number;
    if (text.endsWith('m'))
        return number * 60_000;
    if (text.endsWith('s'))
        return number * 1000;

    return number;
}

function formatMaybeNumber(value, suffix = '') {
    if (!Number.isFinite(value))
        return '-';

    return `${value.toFixed(value >= 10 ? 0 : 1)}${suffix}`;
}

function compareStrings(left, right) {
    return left.localeCompare(right, undefined, { numeric: true, sensitivity: 'base' });
}

function isResultHeading(line) {
    return /^=== .+ \([^)]*\) ===$/.test(line);
}

function parseResultHeading(line) {
    const match = line.match(/^=== (.+) \(([^)]*)\) ===$/);
    if (!match)
        return null;

    return {
        title: match[1],
        metricsText: match[2],
    };
}

function parseMetricNumber(metricsText, regex) {
    const match = metricsText.match(regex);
    if (!match)
        return null;

    const value = Number.parseFloat(match[1]);
    return Number.isFinite(value) ? value : null;
}

function parseMetrics(metricsText) {
    const elapsedText = metricsText.split(',')[0]?.trim() ?? '';
    return {
        raw: metricsText,
        elapsedText,
        elapsedMs: parseDurationToMs(elapsedText),
        samples: parseMetricNumber(metricsText, /samples\s+(\d+)/),
        cpuPercent: parseMetricNumber(metricsText, /cpu\s+([0-9.]+)%/),
        ram: metricsText.match(/ram\s+([^,]+)/)?.[1]?.trim() ?? null,
        gpuPercent: parseMetricNumber(metricsText, /gpu\s+([0-9.]+)%/),
        vram: metricsText.match(/vram\s+([^,]+)/)?.[1]?.trim() ?? null,
        gpuPowerWatts: parseMetricNumber(metricsText, /gpu power\s+([0-9.]+)\s+W/),
        gpuTempCelsius: parseMetricNumber(metricsText, /gpu temp\s+([0-9.]+)\s+C/),
    };
}

function parseResultTitle(title, reportPrompt, transcriptNames) {
    const parts = title.split(' / ');
    const fallbackTranscript = transcriptNames.length === 1 ? transcriptNames[0] : 'transcript-1';

    if (parts.length >= 3) {
        return {
            model: parts[0],
            prompt: parts[1],
            transcript: parts.slice(2).join(' / '),
        };
    }

    if (parts.length === 2) {
        if (parts[1] === reportPrompt || reportPrompt !== 'chirper') {
            return {
                model: parts[0],
                prompt: parts[1],
                transcript: fallbackTranscript,
            };
        }

        return {
            model: parts[0],
            prompt: reportPrompt || 'chirper',
            transcript: parts[1],
        };
    }

    return {
        model: title,
        prompt: reportPrompt || 'chirper',
        transcript: fallbackTranscript,
    };
}

function readIndentedKeyValues(lines, index) {
    const values = {};
    let cursor = index;

    while (cursor < lines.length) {
        const line = lines[cursor];
        if (!line.trim()) {
            cursor += 1;
            break;
        }

        const match = line.match(/^\s{2}([^:]+):\s*(.*)$/);
        if (!match)
            break;

        values[match[1].trim()] = match[2].trim();
        cursor += 1;
    }

    return { values, cursor };
}

function collectUntil(lines, index, stopPredicate) {
    const collected = [];
    let cursor = index;

    while (cursor < lines.length && !stopPredicate(lines[cursor], cursor)) {
        collected.push(lines[cursor]);
        cursor += 1;
    }

    return { text: collected.join('\n').trim(), cursor };
}

function parseTranscriptSections(lines, index) {
    const transcripts = new Map();
    let cursor = index;
    let currentName = null;
    let currentLines = [];

    const flush = () => {
        if (currentName)
            transcripts.set(currentName, currentLines.join('\n').trim());
    };

    while (cursor < lines.length) {
        const line = lines[cursor];
        if (isResultHeading(line))
            break;

        const match = line.match(/^--- (.+) ---$/);
        if (match) {
            flush();
            currentName = match[1].trim();
            currentLines = [];
            cursor += 1;
            continue;
        }

        if (currentName)
            currentLines.push(line);

        cursor += 1;
    }

    flush();
    return { transcripts, cursor };
}

function parseReportFile(path) {
    const text = textFromFile(path);
    const lines = text.split(/\r?\n/);
    const report = {
        path,
        fileName: shortPath(path),
        generatedUnixSeconds: null,
        prompt: 'chirper',
        summary: '',
        mode: '',
        promptInput: '',
        promptElapsedMs: null,
        fullRunElapsedMs: null,
        customPrompt: '',
        promptNote: '',
        hardware: {},
        transcripts: new Map(),
        results: [],
    };

    let index = 0;
    while (index < lines.length) {
        const line = lines[index];

        if (line.startsWith('generated_unix_seconds:')) {
            report.generatedUnixSeconds = Number.parseInt(line.split(':').slice(1).join(':').trim(), 10);
            index += 1;
        } else if (line.startsWith('prompt:')) {
            report.prompt = line.split(':').slice(1).join(':').trim() || 'chirper';
            index += 1;
        } else if (line.startsWith('Tested ')) {
            report.summary = line.trim();
            index += 1;
        } else if (line.startsWith('prompt_elapsed_ms:')) {
            report.promptElapsedMs = Number.parseInt(line.split(':').slice(1).join(':').trim(), 10);
            index += 1;
        } else if (line.startsWith('full_run_elapsed_ms:')) {
            report.fullRunElapsedMs = Number.parseInt(line.split(':').slice(1).join(':').trim(), 10);
            index += 1;
        } else if (line.startsWith('total_elapsed_ms:')) {
            report.fullRunElapsedMs = Number.parseInt(line.split(':').slice(1).join(':').trim(), 10);
            index += 1;
        } else if (line.startsWith('mode:')) {
            report.mode = line.split(':').slice(1).join(':').trim();
            index += 1;
        } else if (line.startsWith('prompt_input:')) {
            report.promptInput = line.split(':').slice(1).join(':').trim();
            index += 1;
        } else if (line === 'prompt_note:') {
            const result = collectUntil(lines, index + 1, next => next === '' || next === 'Custom prompt template:' || next === 'Hardware:');
            report.promptNote = result.text;
            index = result.cursor;
        } else if (line === 'Custom prompt template:') {
            const result = collectUntil(lines, index + 1, next => next === 'Hardware:');
            report.customPrompt = result.text;
            index = result.cursor;
        } else if (line === 'Hardware:') {
            const result = readIndentedKeyValues(lines, index + 1);
            report.hardware = result.values;
            index = result.cursor;
        } else if (line === 'Transcripts:') {
            const result = parseTranscriptSections(lines, index + 1);
            report.transcripts = result.transcripts;
            index = result.cursor;
        } else if (line === 'Raw transcript:') {
            const result = collectUntil(lines, index + 1, next => next.startsWith('Preprocessed draft') || isResultHeading(next));
            report.transcripts.set('transcript-1', result.text);
            index = result.cursor;
        } else if (line.startsWith('Preprocessed draft')) {
            const result = collectUntil(lines, index + 1, next => isResultHeading(next));
            report.preprocessed = result.text;
            index = result.cursor;
        } else if (isResultHeading(line)) {
            const heading = parseResultHeading(line);
            const result = collectUntil(lines, index + 1, next => isResultHeading(next));
            const transcriptNames = [...report.transcripts.keys()];
            const nameParts = parseResultTitle(heading.title, report.prompt, transcriptNames);
            const metrics = parseMetrics(heading.metricsText);
            const output = result.text;

            report.results.push({
                id: `${path}:${report.results.length}`,
                fileName: report.fileName,
                filePath: path,
                report,
                title: heading.title,
                model: nameParts.model,
                prompt: nameParts.prompt,
                transcript: nameParts.transcript,
                metrics,
                output,
                ok: !output.startsWith('ERROR:'),
            });
            index = result.cursor;
        } else {
            index += 1;
        }
    }

    return report;
}

function uniqueSorted(values) {
    return [...new Set(values.filter(Boolean))].sort(compareStrings);
}

function markdownInlineToPango(text) {
    let output = GLib.markup_escape_text(text, -1);
    output = output.replace(/`([^`]+)`/g, '<tt>$1</tt>');
    output = output.replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>');
    output = output.replace(/\*([^*\n]+)\*/g, '<i>$1</i>');
    return output;
}

function markdownToPango(text) {
    const lines = text.trim().split(/\r?\n/);
    const rendered = [];
    let inCode = false;

    for (const line of lines) {
        if (line.trim().startsWith('```')) {
            inCode = !inCode;
            continue;
        }

        if (inCode) {
            rendered.push(`<tt>${GLib.markup_escape_text(line, -1)}</tt>`);
            continue;
        }

        const heading = line.match(/^(#{1,6})\s+(.+)$/);
        if (heading) {
            rendered.push(`<b>${markdownInlineToPango(heading[2])}</b>`);
            continue;
        }

        const bullet = line.match(/^(\s*)[-*]\s+(.+)$/);
        if (bullet) {
            const indent = '  '.repeat(Math.floor(bullet[1].length / 2));
            rendered.push(`${indent}\u2022 ${markdownInlineToPango(bullet[2])}`);
            continue;
        }

        rendered.push(markdownInlineToPango(line));
    }

    return rendered.join('\n');
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

function makeTextView(text, monospace) {
    const buffer = new Gtk.TextBuffer();
    buffer.set_text(text, -1);
    const view = new Gtk.TextView({
        buffer,
        editable: false,
        cursor_visible: false,
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

const ReportViewerWindow = GObject.registerClass(class ReportViewerWindow extends Adw.ApplicationWindow {
    constructor(application) {
        super({
            application,
            title: 'Chirper Report Viewer',
            default_width: 1500,
            default_height: 960,
        });

        this._reports = [];
        this._results = [];
        this._filteredResults = [];
        this._selectLabels = {
            prompt: ['All Prompts'],
            transcript: ['All Transcripts'],
            model: ['All Models'],
        };

        this._build();
        this._loadReports();
    }

    _build() {
        const toolbarView = new Adw.ToolbarView();
        const header = new Adw.HeaderBar({
            title_widget: new Adw.WindowTitle({
                title: 'Report Viewer',
                subtitle: 'Compare Chirper model outputs',
            }),
        });
        this._reloadHeaderButton = new Gtk.Button({ icon_name: 'view-refresh-symbolic' });
        this._reloadHeaderButton.set_tooltip_text('Reload reports');
        this._reloadHeaderButton.connect('clicked', () => this._loadReports());
        header.pack_end(this._reloadHeaderButton);
        toolbarView.add_top_bar(header);

        const split = new Gtk.Paned({
            orientation: Gtk.Orientation.HORIZONTAL,
            shrink_start_child: false,
            shrink_end_child: false,
            resize_start_child: false,
            resize_end_child: true,
        });
        split.set_start_child(this._buildSidebar());
        split.set_end_child(this._buildResultsPane());
        split.set_position(360);

        toolbarView.set_content(split);
        this.set_content(toolbarView);
    }

    _buildSidebar() {
        const scroller = new Gtk.ScrolledWindow({
            hscrollbar_policy: Gtk.PolicyType.NEVER,
            min_content_width: 340,
        });
        const sidebar = new Gtk.Box({
            orientation: Gtk.Orientation.VERTICAL,
            spacing: 14,
            margin_top: 16,
            margin_bottom: 16,
            margin_start: 16,
            margin_end: 12,
        });
        scroller.set_child(sidebar);

        const sourceGroup = new Adw.PreferencesGroup({ title: 'Source' });
        sidebar.append(sourceGroup);
        this._folderRow = new Adw.EntryRow({
            title: 'Report Folder',
            text: defaultReportDir(),
        });
        sourceGroup.add(this._folderRow);

        const reloadRow = new Adw.ActionRow({
            title: 'Load Reports',
            subtitle: 'Reads chirper-format-compare text files from the folder.',
        });
        this._reloadButton = makeButton('Reload', () => this._loadReports(), 'suggested-action');
        reloadRow.add_suffix(this._reloadButton);
        reloadRow.activatable_widget = this._reloadButton;
        sourceGroup.add(reloadRow);

        const filterGroup = new Adw.PreferencesGroup({ title: 'Filters' });
        sidebar.append(filterGroup);

        this._promptRow = new Adw.ComboRow({
            title: 'Prompt',
            model: makeStringList(this._selectLabels.prompt),
            selected: 0,
        });
        this._promptRow.connect('notify::selected', () => this._applyFilters());
        filterGroup.add(this._promptRow);

        this._transcriptRow = new Adw.ComboRow({
            title: 'Transcript',
            model: makeStringList(this._selectLabels.transcript),
            selected: 0,
        });
        this._transcriptRow.connect('notify::selected', () => this._applyFilters());
        filterGroup.add(this._transcriptRow);

        this._modelRow = new Adw.ComboRow({
            title: 'Model',
            model: makeStringList(this._selectLabels.model),
            selected: 0,
        });
        this._modelRow.connect('notify::selected', () => this._applyFilters());
        filterGroup.add(this._modelRow);

        this._searchRow = new Adw.EntryRow({
            title: 'Search Output',
        });
        this._searchRow.connect('changed', () => this._applyFilters());
        filterGroup.add(this._searchRow);

        const viewGroup = new Adw.PreferencesGroup({ title: 'View' });
        sidebar.append(viewGroup);

        this._viewRow = new Adw.ComboRow({
            title: 'Output View',
            model: makeStringList(VIEW_OPTIONS.map(([, label]) => label)),
            selected: 0,
        });
        this._viewRow.connect('notify::selected', () => this._renderResults());
        viewGroup.add(this._viewRow);

        this._sortRow = new Adw.ComboRow({
            title: 'Sort By',
            model: makeStringList(SORT_OPTIONS.map(([, label]) => label)),
            selected: 0,
        });
        this._sortRow.connect('notify::selected', () => this._applyFilters());
        viewGroup.add(this._sortRow);

        this._showFailuresRow = new Adw.SwitchRow({
            title: 'Show Errors',
            active: true,
        });
        this._showFailuresRow.connect('notify::active', () => this._applyFilters());
        viewGroup.add(this._showFailuresRow);

        const metaGroup = new Adw.PreferencesGroup({ title: 'Summary' });
        sidebar.append(metaGroup);
        this._statusRow = new Adw.ActionRow({
            title: 'Status',
            subtitle: 'Loading reports',
        });
        metaGroup.add(this._statusRow);
        this._countsRow = new Adw.ActionRow({
            title: 'Loaded',
            subtitle: '-',
        });
        metaGroup.add(this._countsRow);
        this._hardwareRow = new Adw.ActionRow({
            title: 'Hardware',
            subtitle: '-',
        });
        metaGroup.add(this._hardwareRow);

        return scroller;
    }

    _buildResultsPane() {
        const wrapper = new Gtk.Box({
            orientation: Gtk.Orientation.VERTICAL,
            spacing: 10,
            margin_top: 16,
            margin_bottom: 16,
            margin_start: 12,
            margin_end: 16,
        });

        this._resultTitle = new Gtk.Label({
            label: 'Results',
            xalign: 0,
        });
        this._resultTitle.add_css_class('title-2');
        wrapper.append(this._resultTitle);

        this._resultSubTitle = new Gtk.Label({
            label: 'Loading',
            xalign: 0,
            wrap: true,
        });
        this._resultSubTitle.add_css_class('dim-label');
        wrapper.append(this._resultSubTitle);

        const scroller = new Gtk.ScrolledWindow({
            hscrollbar_policy: Gtk.PolicyType.NEVER,
            vexpand: true,
        });
        this._flow = new Gtk.FlowBox({
            selection_mode: Gtk.SelectionMode.NONE,
            column_spacing: 12,
            row_spacing: 12,
            homogeneous: false,
            min_children_per_line: 1,
            max_children_per_line: 4,
        });
        this._flow.set_valign(Gtk.Align.START);
        scroller.set_child(this._flow);
        wrapper.append(scroller);

        return wrapper;
    }

    _setComboLabels(row, key, labels, allLabel) {
        const previous = this._selectLabels[key]?.[row.selected] ?? allLabel;
        this._selectLabels[key] = [allLabel, ...labels];
        row.set_model(makeStringList(this._selectLabels[key]));
        const nextIndex = Math.max(0, this._selectLabels[key].indexOf(previous));
        row.set_selected(nextIndex);
    }

    _loadReports() {
        const folder = this._folderRow.text.trim() || defaultReportDir();
        this._statusRow.subtitle = 'Reading report files';
        this._reports = [];
        this._results = [];

        try {
            const paths = listReportFiles(folder);
            for (const path of paths) {
                const report = parseReportFile(path);
                this._reports.push(report);
                this._results.push(...report.results);
            }

            this._reports.sort((left, right) => compareStrings(left.fileName, right.fileName));
            this._results.sort((left, right) => compareStrings(left.model, right.model));
            this._refreshFilterOptions();
            this._statusRow.subtitle = `Loaded ${paths.length} report files`;
            this._applyFilters();
        } catch (error) {
            this._statusRow.subtitle = error.message;
            this._reports = [];
            this._results = [];
            this._refreshFilterOptions();
            this._applyFilters();
        }
    }

    _refreshFilterOptions() {
        this._setComboLabels(
            this._promptRow,
            'prompt',
            uniqueSorted(this._results.map(result => result.prompt)),
            'All Prompts'
        );
        this._setComboLabels(
            this._transcriptRow,
            'transcript',
            uniqueSorted(this._results.map(result => result.transcript)),
            'All Transcripts'
        );
        this._setComboLabels(
            this._modelRow,
            'model',
            uniqueSorted(this._results.map(result => result.model)),
            'All Models'
        );

        const hardware = this._reports.find(report => Object.keys(report.hardware).length > 0)?.hardware;
        this._hardwareRow.subtitle = hardware
            ? [hardware.cpu, hardware.gpu_name, hardware.gpu_vram_total ? `${hardware.gpu_vram_total} VRAM` : null]
                .filter(Boolean)
                .join(' | ')
            : '-';
        this._countsRow.subtitle = `${this._reports.length} files, ${this._results.length} results`;
    }

    _selectedFilter(row, key) {
        const value = this._selectLabels[key]?.[row.selected] ?? '';
        return value.startsWith('All ') ? null : value;
    }

    _applyFilters() {
        const prompt = this._selectedFilter(this._promptRow, 'prompt');
        const transcript = this._selectedFilter(this._transcriptRow, 'transcript');
        const model = this._selectedFilter(this._modelRow, 'model');
        const search = this._searchRow.text.trim().toLowerCase();
        const showFailures = this._showFailuresRow.active;

        this._filteredResults = this._results.filter(result => {
            if (prompt && result.prompt !== prompt)
                return false;
            if (transcript && result.transcript !== transcript)
                return false;
            if (model && result.model !== model)
                return false;
            if (!showFailures && !result.ok)
                return false;
            if (search) {
                const haystack = `${result.model}\n${result.prompt}\n${result.transcript}\n${result.output}`.toLowerCase();
                if (!haystack.includes(search))
                    return false;
            }
            return true;
        });

        this._sortResults();
        this._renderResults();
    }

    _sortResults() {
        const mode = SORT_OPTIONS[this._sortRow.selected]?.[0] ?? 'model';
        const numberValue = (value) => Number.isFinite(value) ? value : Number.POSITIVE_INFINITY;

        this._filteredResults.sort((left, right) => {
            switch (mode) {
            case 'prompt':
                return compareStrings(left.prompt, right.prompt) ||
                    compareStrings(left.transcript, right.transcript) ||
                    compareStrings(left.model, right.model);
            case 'transcript':
                return compareStrings(left.transcript, right.transcript) ||
                    compareStrings(left.prompt, right.prompt) ||
                    compareStrings(left.model, right.model);
            case 'runtime':
                return numberValue(left.metrics.elapsedMs) - numberValue(right.metrics.elapsedMs);
            case 'gpu':
                return numberValue(right.metrics.gpuPercent) - numberValue(left.metrics.gpuPercent);
            case 'power':
                return numberValue(right.metrics.gpuPowerWatts) - numberValue(left.metrics.gpuPowerWatts);
            case 'file':
                return compareStrings(left.fileName, right.fileName) ||
                    compareStrings(left.model, right.model);
            case 'model':
            default:
                return compareStrings(left.model, right.model) ||
                    compareStrings(left.prompt, right.prompt) ||
                    compareStrings(left.transcript, right.transcript);
            }
        });
    }

    _renderResults() {
        while (this._flow.get_first_child())
            this._flow.remove(this._flow.get_first_child());

        const prompt = this._selectedFilter(this._promptRow, 'prompt') ?? 'all prompts';
        const transcript = this._selectedFilter(this._transcriptRow, 'transcript') ?? 'all transcripts';
        const model = this._selectedFilter(this._modelRow, 'model') ?? 'all models';
        this._resultTitle.label = `${this._filteredResults.length} Results`;
        this._resultSubTitle.label = `${model} | ${prompt} | ${transcript}`;

        if (this._filteredResults.length === 0) {
            const empty = new Gtk.Label({
                label: 'No matching results',
                xalign: 0,
                margin_top: 24,
                margin_start: 12,
            });
            empty.add_css_class('dim-label');
            this._flow.append(empty);
            return;
        }

        const viewMode = VIEW_OPTIONS[this._viewRow.selected]?.[0] ?? 'regular';
        for (const result of this._filteredResults)
            this._flow.append(this._buildResultCard(result, viewMode));
    }

    _buildResultCard(result, viewMode) {
        const card = new Gtk.Box({
            orientation: Gtk.Orientation.VERTICAL,
            spacing: 8,
            margin_top: 2,
            margin_bottom: 2,
            margin_start: 2,
            margin_end: 2,
            width_request: 430,
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
            label: result.model,
            xalign: 0,
            ellipsize: Pango.EllipsizeMode.END,
        });
        title.add_css_class('heading');
        header.append(title);

        const context = new Gtk.Label({
            label: `${result.prompt} | ${result.transcript}`,
            xalign: 0,
            wrap: true,
        });
        context.add_css_class('dim-label');
        header.append(context);

        const metricLine = [
            result.metrics.elapsedText,
            `GPU ${formatMaybeNumber(result.metrics.gpuPercent, '%')}`,
            `VRAM ${result.metrics.vram ?? '-'}`,
            `${formatMaybeNumber(result.metrics.gpuPowerWatts, ' W')}`,
        ].filter(Boolean).join('  |  ');
        const metrics = new Gtk.Label({
            label: metricLine,
            xalign: 0,
            wrap: true,
        });
        metrics.add_css_class(result.ok ? 'caption' : 'error');
        header.append(metrics);

        const file = new Gtk.Label({
            label: result.fileName,
            xalign: 0,
            ellipsize: Pango.EllipsizeMode.END,
        });
        file.add_css_class('caption');
        file.add_css_class('dim-label');
        header.append(file);

        const outputScroller = new Gtk.ScrolledWindow({
            min_content_height: 270,
            max_content_height: 420,
            hscrollbar_policy: Gtk.PolicyType.NEVER,
            margin_bottom: 12,
            margin_start: 12,
            margin_end: 12,
        });
        outputScroller.add_css_class('view');

        if (viewMode === 'markdown') {
            const label = new Gtk.Label({
                label: markdownToPango(result.output || ''),
                use_markup: true,
                selectable: true,
                wrap: true,
                xalign: 0,
                yalign: 0,
                margin_top: 10,
                margin_bottom: 10,
                margin_start: 10,
                margin_end: 10,
            });
            outputScroller.set_child(label);
        } else {
            outputScroller.set_child(makeTextView(result.output || '', true));
        }

        card.append(outputScroller);

        const transcriptText = result.report.transcripts.get(result.transcript);
        if (transcriptText) {
            const source = new Gtk.Label({
                label: `Source: ${compactText(transcriptText)}`,
                xalign: 0,
                wrap: true,
                margin_bottom: 12,
                margin_start: 12,
                margin_end: 12,
            });
            source.add_css_class('caption');
            source.add_css_class('dim-label');
            card.append(source);
        }

        return card;
    }
});

const app = new Adw.Application({
    application_id: 'dev.local.Chirper.ReportViewer',
    flags: Gio.ApplicationFlags.FLAGS_NONE,
});

app.connect('activate', application => {
    window = new ReportViewerWindow(application);
    window.present();
});

app.run([GLib.get_prgname() ?? 'chirper-report-viewer']);
