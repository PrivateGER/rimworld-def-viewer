async function loadCompressedJson(fileName, description) {
    try {
        const response = await fetch(fileName);
        if (!response.ok) {
            throw new Error(`Failed to fetch ${description}: ${response.status} ${response.statusText}`);
        }

        const compressed = new Uint8Array(await response.arrayBuffer());

        // Decompress with zstd
        const decompressed = fzstd.decompress(compressed);

        // Convert to string and parse JSON
        const jsonString = new TextDecoder().decode(decompressed);
        return JSON.parse(jsonString);
    } catch (error) {
        console.error(`Failed to load ${description}:`, error);
        throw new Error(`Failed to load ${description}: ${error.message}`);
    }
}

function loadDataFromFile() {
    return loadCompressedJson('dataset.json.zstd', 'definition data');
}

function loadRawXmlFromFile() {
    return loadCompressedJson('raw-xml.json.zstd', 'raw XML');
}

function extensionFromFilePath(filePath) {
    const pathParts = filePath.split(/[\\/]/);
    return pathParts[0] === 'Data' && pathParts[1] ? pathParts[1] : 'Unknown';
}

const SEARCH_MIN_LENGTH = 3;
const SEARCH_DEBOUNCE_MS = 150;
const SEARCH_RESULT_BATCH_SIZE = 50;

const { createApp } = Vue;

createApp({
    data() {
        return {
            // Data
            categories: [],
            stats: { total_defs: 0, total_categories: 0, total_files: 0 },
            defsById: {},
            definitionIndex: [],
            rawXmlById: null,
            rawXmlLoadPromise: null,

            // Filters and Search
            searchInput: '',
            searchQuery: '',
            typeFilter: 'all',
            extensionFilter: 'all',
            searchResultLimit: SEARCH_RESULT_BATCH_SIZE,
            searchDebounceTimer: null,

            // UI State
            activeCategory: 'overview',
            expandedDefs: new Set(),
            mobileMenuOpen: false,
            xmlDefinitionId: null,
            xmlReturnFocus: null,
            copyStatus: '',
            copyStatusTimer: null,

            // Loading State
            loading: true,
            error: null,
            rawXmlLoadingIds: new Set(),
            rawXmlLoadError: null
        }
    },
    computed: {
        trimmedSearchQuery() {
            return this.searchQuery.trim().toLowerCase();
        },
        isShortSearch() {
            const length = this.trimmedSearchQuery.length;
            return length > 0 && length < SEARCH_MIN_LENGTH;
        },
        searchCharactersNeeded() {
            return Math.max(0, SEARCH_MIN_LENGTH - this.trimmedSearchQuery.length);
        },
        hasActiveFilters() {
            return Boolean(
                this.trimmedSearchQuery ||
                this.typeFilter !== 'all' ||
                this.extensionFilter !== 'all'
            );
        },
        searchResults() {
            const query = this.trimmedSearchQuery;
            if (query && query.length < SEARCH_MIN_LENGTH) {
                return [];
            }

            return this.definitionIndex
                .filter(entry => this._matchesSelectedFilters(entry.definition))
                .map(entry => ({ entry, rank: query ? this._searchRank(entry, query) : 0 }))
                .filter(result => result.rank !== null)
                .sort((left, right) => left.rank - right.rank || left.entry.order - right.entry.order)
                .map(result => result.entry.definition);
        },
        visibleSearchResults() {
            return this.searchResults.slice(0, this.searchResultLimit);
        },
        hasMoreSearchResults() {
            return this.visibleSearchResults.length < this.searchResults.length;
        },
        searchResultIds() {
            return new Set(this.searchResults.map(definition => definition.id));
        },
        searchResultCountsByCategory() {
            const counts = new Map();
            for (const definition of this.searchResults) {
                counts.set(definition.def_type, (counts.get(definition.def_type) || 0) + 1);
            }
            return counts;
        },
        visibleCategories() {
            if (!this.hasActiveFilters) {
                return this.categories;
            }

            return this.categories.filter(category => this.searchResultCountsByCategory.has(category.name));
        },
        displayedCategories() {
            if (this.activeCategory === 'overview' || this.activeCategory === 'all') {
                return [];
            }

            const category = this.visibleCategories.find(
                candidate => candidate.name === this.activeCategory
            );
            return category ? [category] : [];
        },
        filteredDefinitionsCount() {
            if (this.activeCategory === 'all') {
                return this.searchResults.length;
            }
            return this.visibleCategories.reduce((total, category) => {
                return total + this.getFilteredDefinitions(category).length;
            }, 0);
        },
        xmlDefinition() {
            return this.definitionById(this.xmlDefinitionId);
        },
        hasUnknownExtension() {
            return this.categories.some(category =>
                category.definitions.some(def => def.extension === 'Unknown')
            );
        }
    },
    methods: {
        async loadData() {
            try {
                this.loading = true;
                this.error = null;

                const data = await loadDataFromFile();
                this.categories = data.categories;
                this.stats = data.stats;

                // Build definition ID map for navigation
                this.defsById = {};
                for (const category of this.categories) {
                    for (const def of category.definitions) {
                        def.def_type = category.name;
                        def.extension = extensionFromFilePath(def.file_path);
                        def.is_abstract = def.is_abstract === true;
                        def.tags ||= [];
                        def.references_out ||= [];
                        def.references_in ||= [];
                        def.code_references ||= [];
                        const defInfo = { def, category: category.name };
                        this.defsById[def.id] = defInfo;
                    }
                }
                this.rebuildDefinitionIndex();

                // Rebuild unique reverse edges while keeping references as IDs.
                for (const category of this.categories) {
                    for (const def of category.definitions) {
                        for (const reference of def.references_out) {
                            if (reference.kind !== 'heuristic' && reference.targets.length === 1) {
                                const target = this.definitionById(reference.targets[0]);
                                if (target && !target.references_in.includes(def.id)) {
                                    target.references_in.push(def.id);
                                }
                            }
                        }
                    }
                }

                this.loading = false;
                return Promise.resolve();
            } catch (error) {
                console.error('Load error:', error);
                this.error = error.message;
                this.loading = false;
                return Promise.reject(error);
            }
        },
        retryLoadData() {
            return this.loadData().then(() => this.applyHashOnLoad());
        },
        rebuildDefinitionIndex() {
            let order = 0;
            this.definitionIndex = this.categories.flatMap(category =>
                category.definitions.map(definition => this._createSearchEntry(definition, order++))
            );
        },
        _createSearchEntry(definition, order) {
            const normalize = value => String(value || '').toLowerCase();
            return {
                definition,
                order,
                defName: normalize(definition.def_name),
                inheritanceName: normalize(definition.inheritance_name),
                label: normalize(definition.label),
                description: normalize(definition.description),
                tags: (definition.tags || []).map(normalize).join(' '),
                defType: normalize(definition.def_type)
            };
        },
        _searchRank(entry, query) {
            if (entry.defName === query) {
                return 0;
            }
            if (entry.defName.startsWith(query) || entry.inheritanceName.startsWith(query)) {
                return 1;
            }
            if (entry.defName.includes(query) || entry.inheritanceName.includes(query) || entry.label.includes(query)) {
                return 2;
            }
            if (entry.description.includes(query) || entry.tags.includes(query) || entry.defType.includes(query)) {
                return 3;
            }
            return null;
        },
        _matchesSelectedFilters(definition) {
            if (this.typeFilter === 'abstract' && !definition.is_abstract) {
                return false;
            }
            if (this.typeFilter === 'concrete' && definition.is_abstract) {
                return false;
            }
            return this.extensionFilter === 'all' || definition.extension === this.extensionFilter;
        },
        setActiveCategory(category) {
            this.activeCategory = category;
            this.closeMobileMenu();
        },
        toggleMobileMenu() {
            this.mobileMenuOpen = !this.mobileMenuOpen;
        },
        closeMobileMenu() {
            this.mobileMenuOpen = false;
        },
        setTypeFilter(filter) {
            this.typeFilter = filter;
            this.searchResultLimit = SEARCH_RESULT_BATCH_SIZE;
            this._syncFilteredCategory();
        },
        setExtensionFilter(filter) {
            this.extensionFilter = filter;
            this.searchResultLimit = SEARCH_RESULT_BATCH_SIZE;
            this._syncFilteredCategory();
        },
        getFilteredDefinitions(category) {
            if (!this.hasActiveFilters) {
                return category.definitions;
            }
            return category.definitions.filter(definition => this.searchResultIds.has(definition.id));
        },
        getVisibleCount(category) {
            if (!this.hasActiveFilters) {
                return category.definitions.length;
            }
            return this.searchResultCountsByCategory.get(category.name) || 0;
        },
        definitionById(definitionId) {
            return this.defsById[definitionId]?.def;
        },
        definitionDisplayName(definition) {
            return definition?.def_name || definition?.inheritance_name || 'Unnamed definition';
        },
        referencesByKind(definition, kind) {
            return definition.references_out.filter(reference => reference.kind === kind);
        },
        parentReference(definition) {
            return definition.references_out.find(reference => reference.kind === 'parent');
        },
        performSearch() {
            clearTimeout(this.searchDebounceTimer);
            this.searchDebounceTimer = setTimeout(() => this.applySearch(), SEARCH_DEBOUNCE_MS);
        },
        applySearch() {
            clearTimeout(this.searchDebounceTimer);
            this.searchDebounceTimer = null;
            this.searchQuery = this.searchInput.trim();
            this.searchResultLimit = SEARCH_RESULT_BATCH_SIZE;
            this._syncFilteredCategory();
        },
        clearSearch() {
            this.searchInput = '';
            this.searchQuery = '';
            this.searchResultLimit = SEARCH_RESULT_BATCH_SIZE;
            this._syncFilteredCategory();
        },
        clearAllFilters() {
            this.searchInput = '';
            this.searchQuery = '';
            this.typeFilter = 'all';
            this.extensionFilter = 'all';
            this.searchResultLimit = SEARCH_RESULT_BATCH_SIZE;
            this._syncFilteredCategory();
        },
        showMoreSearchResults() {
            this.searchResultLimit += SEARCH_RESULT_BATCH_SIZE;
        },
        _syncFilteredCategory() {
            if (this.hasActiveFilters) {
                this.activeCategory = 'all';
            } else if (this.activeCategory === 'all') {
                this.activeCategory = 'overview';
            }
        },
        // Toggle Methods
        toggleDef(definitionId) {
            this.expandedDefs = this._toggleSet(this.expandedDefs, definitionId);
        },
        async openXML(definitionId, event) {
            this.rawXmlLoadingIds = this._toggleSet(this.rawXmlLoadingIds, definitionId, true);
            if (this.rawXmlLoadError?.definitionId === definitionId) {
                this.rawXmlLoadError = null;
            }
            try {
                if (!this.rawXmlById) {
                    this.rawXmlLoadPromise ||= loadRawXmlFromFile();
                    this.rawXmlById = await this.rawXmlLoadPromise;
                    this.rawXmlLoadPromise = null;
                }
                if (!Object.hasOwn(this.rawXmlById, definitionId)) {
                    throw new Error(`Raw XML not found for ${definitionId}`);
                }
                this.xmlReturnFocus = event?.currentTarget || null;
                this.xmlDefinitionId = definitionId;
                this.copyStatus = '';
                this.$nextTick?.(() => this.$refs?.xmlCloseButton?.focus());
            } catch (error) {
                this.rawXmlLoadPromise = null;
                this.rawXmlLoadError = { definitionId, message: error.message };
            } finally {
                this.rawXmlLoadingIds = this._toggleSet(this.rawXmlLoadingIds, definitionId, false);
            }
        },
        closeXML() {
            const returnFocus = this.xmlReturnFocus;
            this.xmlDefinitionId = null;
            this.copyStatus = '';
            this.xmlReturnFocus = null;
            this.$nextTick?.(() => returnFocus?.focus());
        },
        _toggleSet(set, item, force) {
            const newSet = new Set(set);
            if (force === true) {
                newSet.add(item);
            } else if (force === false || newSet.has(item)) {
                newSet.delete(item);
            } else {
                newSet.add(item);
            }
            return newSet;
        },
        formatXML(xml) {
            // First, escape HTML entities
            const escaped = xml
                .replace(/&/g, '&amp;')
                .replace(/</g, '&lt;')
                .replace(/>/g, '&gt;')
                .replace(/"/g, '&quot;');

            // Now apply syntax highlighting
            let result = '';
            let i = 0;

            while (i < escaped.length) {
                // Handle comments
                if (escaped.substring(i).startsWith('&lt;!--')) {
                    const commentEnd = escaped.indexOf('--&gt;', i);
                    if (commentEnd !== -1) {
                        result += '<span class="xml-comment">' + escaped.substring(i, commentEnd + 6) + '</span>';
                        i = commentEnd + 6;
                        continue;
                    }
                }

                // Handle CDATA
                if (escaped.substring(i).startsWith('&lt;![CDATA[')) {
                    const cdataEnd = escaped.indexOf(']]&gt;', i);
                    if (cdataEnd !== -1) {
                        result += '<span class="xml-cdata">' + escaped.substring(i, cdataEnd + 6) + '</span>';
                        i = cdataEnd + 6;
                        continue;
                    }
                }

                // Handle XML declaration
                if (escaped.substring(i).startsWith('&lt;?xml')) {
                    const declEnd = escaped.indexOf('?&gt;', i);
                    if (declEnd !== -1) {
                        result += '<span class="xml-declaration">' + escaped.substring(i, declEnd + 5) + '</span>';
                        i = declEnd + 5;
                        continue;
                    }
                }

                // Handle tags
                if (escaped[i] === '&' && escaped.substring(i).startsWith('&lt;')) {
                    const tagEnd = escaped.indexOf('&gt;', i);
                    if (tagEnd !== -1) {
                        const tagContent = escaped.substring(i + 4, tagEnd);

                        // Check if it's a closing tag
                        if (tagContent.startsWith('/')) {
                            result += '<span class="xml-tag">&lt;/' + tagContent.substring(1) + '&gt;</span>';
                        } else {
                            // Parse tag name and attributes
                            const spaceIndex = tagContent.search(/\s/);
                            const tagName = spaceIndex === -1 ? tagContent.replace('/', '') : tagContent.substring(0, spaceIndex);
                            const attributesStr = spaceIndex === -1 ? '' : tagContent.substring(spaceIndex);

                            result += '<span class="xml-tag">&lt;' + tagName + '</span>';

                            if (attributesStr) {
                                // Parse attributes
                                const attrRegex = /(\s+)([\w\-:]+)(=)(&quot;)([^&]*?)(&quot;)/g;
                                let processedAttrs = attributesStr.replace(attrRegex, (match, space, attrName, equals, quote1, attrValue, quote2) => {
                                    if (attrName === 'Class') {
                                        return space + '<span class="xml-attr">' + attrName + '</span>' + equals +
                                               '<span class="xml-class-value">' + quote1 + attrValue + quote2 + '</span>';
                                    } else {
                                        return space + '<span class="xml-attr">' + attrName + '</span>' + equals +
                                               '<span class="xml-value">' + quote1 + attrValue + quote2 + '</span>';
                                    }
                                });

                                // Handle self-closing slash
                                processedAttrs = processedAttrs.replace(/(\s*)(\/?)$/, (match, space, slash) => {
                                    return space + (slash ? '<span class="xml-tag">' + slash + '</span>' : '');
                                });

                                result += processedAttrs;
                            }

                            result += '<span class="xml-tag">&gt;</span>';
                        }

                        i = tagEnd + 4;
                        continue;
                    }
                }

                // Handle text content
                if (escaped[i] !== '&' || !escaped.substring(i).startsWith('&lt;')) {
                    let textEnd = i + 1;
                    while (textEnd < escaped.length &&
                           (escaped[textEnd] !== '&' || !escaped.substring(textEnd).startsWith('&lt;'))) {
                        textEnd++;
                    }

                    const text = escaped.substring(i, textEnd).trim();
                    if (text) {
                        result += '<span class="xml-content">' + escaped.substring(i, textEnd) + '</span>';
                    } else {
                        result += escaped.substring(i, textEnd);
                    }
                    i = textEnd;
                    continue;
                }

                // Default case - just append the character
                result += escaped[i];
                i++;
            }

            return result;
        },
        rawXmlFor(definitionId) {
            return this.rawXmlById?.[definitionId] || '';
        },
        async copyXML(xml) {
            let copied = true;
            try {
                await navigator.clipboard.writeText(xml);
            } catch (err) {
                // Fallback for older browsers
                copied = this._fallbackCopy(xml);
            }
            this.copyStatus = copied ? 'XML copied to clipboard.' : 'Unable to copy XML.';
            clearTimeout(this.copyStatusTimer);
            this.copyStatusTimer = setTimeout(() => {
                this.copyStatus = '';
            }, 2000);
        },
        _fallbackCopy(text) {
            const textArea = document.createElement('textarea');
            textArea.value = text;
            textArea.style.position = 'fixed';
            textArea.style.opacity = '0';
            document.body.appendChild(textArea);
            textArea.select();
            let copied = false;
            try {
                copied = document.execCommand('copy');
            } catch (err) {
                console.error('Copy failed:', err);
            }
            document.body.removeChild(textArea);
            return copied;
        },
        handleGlobalKeydown(event) {
            const tagName = event.target?.tagName?.toLowerCase();
            const isControl = ['input', 'textarea', 'select', 'button', 'a'].includes(tagName) || event.target?.isContentEditable;

            if (event.key === '/' && !isControl) {
                event.preventDefault();
                this.$refs?.searchInput?.focus();
                return;
            }

            if (event.key === 'Escape' && this.xmlDefinitionId) {
                event.preventDefault();
                this.closeXML();
                return;
            }

            if (event.key === 'Escape' && !this.xmlDefinitionId && (this.searchInput || this.searchQuery)) {
                event.preventDefault();
                this.clearSearch();
                this.$refs?.searchInput?.focus();
                return;
            }

            if (!this.xmlDefinitionId && this.activeCategory === 'all' && ['ArrowDown', 'ArrowUp'].includes(event.key)) {
                const results = Array.from(document.querySelectorAll('.search-result-summary'));
                if (results.length === 0) {
                    return;
                }
                event.preventDefault();
                const currentIndex = results.indexOf(document.activeElement);
                const delta = event.key === 'ArrowDown' ? 1 : -1;
                const nextIndex = currentIndex === -1
                    ? (delta === 1 ? 0 : results.length - 1)
                    : (currentIndex + delta + results.length) % results.length;
                results[nextIndex].focus();
            }
        },
        handleDrawerKeydown(event) {
            if (event.key === 'Escape') {
                event.preventDefault();
                event.stopPropagation();
                this.closeXML();
                return;
            }
            if (event.key !== 'Tab') {
                return;
            }

            const focusable = Array.from(this.$refs?.xmlDrawer?.querySelectorAll(
                'button:not([disabled]), [href], input, textarea, select, [tabindex]:not([tabindex="-1"])'
            ) || []);
            if (focusable.length === 0) {
                return;
            }
            const first = focusable[0];
            const last = focusable[focusable.length - 1];
            if (event.shiftKey && document.activeElement === first) {
                event.preventDefault();
                last.focus();
            } else if (!event.shiftKey && document.activeElement === last) {
                event.preventDefault();
                first.focus();
            }
        },
        scrollToDefinitionById(definitionId) {
            const defInfo = this.defsById[definitionId];
            if (!defInfo) {
                console.warn(`Definition not found: ${definitionId}`);
                return;
            }

            this.scrollToDefinition(defInfo, true);
        },
        scrollToDefinition(defInfo, updateHash) {
            const definitionId = defInfo.def.id;

            // Set the category to show the definition
            this.activeCategory = defInfo.category;

            // Clear search to ensure def is visible
            this.searchQuery = '';
            this.searchInput = '';
            this.typeFilter = 'all';
            this.extensionFilter = 'all';

            // Collapse all cards and expand only the target definition
            this.expandedDefs.clear();
            this.expandedDefs.add(definitionId);
            this.expandedDefs = new Set(this.expandedDefs);

            // Close the source drawer when navigating to another definition.
            this.xmlDefinitionId = null;

            // Update URL hash
            if (updateHash) {
                window.location.hash = encodeURIComponent(definitionId);
            }

            // Wait for Vue to update the DOM
            this.$nextTick(() => {
                requestAnimationFrame(() => {
                    this._scrollToElement(definitionId);
                });
            });
        },
        handleHashChange() {
            const hash = window.location.hash.slice(1);
            if (!hash) {
                return;
            }

            let definitionId;
            try {
                definitionId = decodeURIComponent(hash);
            } catch (error) {
                console.warn('Invalid definition hash:', hash);
                return;
            }

            const defInfo = this.defsById[definitionId];
            if (defInfo) {
                this.scrollToDefinition(defInfo, false);
            }
        },
        definitionElementId(definitionId) {
            return `def-${encodeURIComponent(definitionId)}`;
        },
        _scrollToElement(definitionId) {
            const element = document.getElementById(this.definitionElementId(definitionId));
            if (element) {
                element.scrollIntoView({ behavior: 'smooth', block: 'center' });
                // Add a highlight effect
                element.style.transition = 'box-shadow 0.3s ease';
                element.style.boxShadow = '0 0 20px var(--primary-color)';
                setTimeout(() => {
                    element.style.boxShadow = '';
                }, 2000);
            }
        },
        applyHashOnLoad() {
            // Apply hash navigation after data is loaded
            if (window.location.hash) {
                setTimeout(() => {
                    this.handleHashChange();
                }, 100);
            }
        }
    },
    mounted() {
        this.loadData().then(() => {
            this.applyHashOnLoad();
        });

        // Listen for hash changes (browser back/forward)
        window.addEventListener('hashchange', this.handleHashChange);
        window.addEventListener('keydown', this.handleGlobalKeydown);
    },
    beforeUnmount() {
        clearTimeout(this.searchDebounceTimer);
        clearTimeout(this.copyStatusTimer);
        window.removeEventListener('hashchange', this.handleHashChange);
        window.removeEventListener('keydown', this.handleGlobalKeydown);
    }
}).mount('#app');
