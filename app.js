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

const { createApp } = Vue;

createApp({
    data() {
        return {
            // Data
            categories: [],
            stats: { total_defs: 0, total_categories: 0, total_files: 0 },
            defsById: {},
            rawXmlById: null,

            // Filters and Search
            searchQuery: '',
            typeFilter: 'all',
            extensionFilter: 'all',

            // UI State
            activeCategory: 'overview',
            expandedDefs: new Set(),
            showXML: new Set(),
            mobileMenuOpen: false,

            // Loading State
            loading: true,
            error: null,
            rawXmlLoadingId: null,
            rawXmlLoadError: null
        }
    },
    computed: {
        hasActiveFilters() {
            return Boolean(
                this.searchQuery ||
                this.typeFilter !== 'all' ||
                this.extensionFilter !== 'all'
            );
        },
        visibleCategories() {
            if (!this.hasActiveFilters) {
                return this.categories;
            }

            return this.categories.filter(category =>
                this.getFilteredDefinitions(category).length > 0
            );
        },
        displayedCategories() {
            if (this.activeCategory === 'overview') {
                return [];
            }
            if (this.activeCategory === 'all') {
                return this.visibleCategories;
            }

            const category = this.visibleCategories.find(
                candidate => candidate.name === this.activeCategory
            );
            return category ? [category] : [];
        },
        filteredDefinitionsCount() {
            return this.visibleCategories.reduce((total, category) => {
                return total + this.getFilteredDefinitions(category).length;
            }, 0);
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
            this._syncFilteredCategory();
        },
        setExtensionFilter(filter) {
            this.extensionFilter = filter;
            this._syncFilteredCategory();
        },
        getFilteredDefinitions(category) {
            let filtered = category.definitions;

            // Apply search filter
            if (this.searchQuery) {
                const query = this.searchQuery.toLowerCase();
                filtered = filtered.filter(def => {
                    return (def.def_name && def.def_name.toLowerCase().includes(query)) ||
                        (def.inheritance_name && def.inheritance_name.toLowerCase().includes(query)) ||
                           (def.label && def.label.toLowerCase().includes(query)) ||
                           (def.description && def.description.toLowerCase().includes(query)) ||
                           (def.tags && def.tags.some(tag => tag.toLowerCase().includes(query))) ||
                           def.def_type.toLowerCase().includes(query);
                });
            }

            // Apply type filter
            if (this.typeFilter === 'abstract') {
                filtered = filtered.filter(def => def.is_abstract);
            } else if (this.typeFilter === 'concrete') {
                filtered = filtered.filter(def => !def.is_abstract);
            }

            // Apply extension filter
            if (this.extensionFilter !== 'all') {
                filtered = filtered.filter(def => def.extension === this.extensionFilter);
            }

            return filtered;
        },
        getVisibleCount(category) {
            return this.getFilteredDefinitions(category).length;
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
            this._syncFilteredCategory();
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
        async toggleXML(definitionId) {
            if (this.showXML.has(definitionId)) {
                this.showXML = this._toggleSet(this.showXML, definitionId);
                return;
            }

            this.rawXmlLoadingId = definitionId;
            this.rawXmlLoadError = null;
            try {
                this.rawXmlById ||= await loadRawXmlFromFile();
                if (!Object.hasOwn(this.rawXmlById, definitionId)) {
                    throw new Error(`Raw XML not found for ${definitionId}`);
                }
                this.showXML = this._toggleSet(this.showXML, definitionId);
            } catch (error) {
                this.rawXmlLoadError = { definitionId, message: error.message };
            } finally {
                this.rawXmlLoadingId = null;
            }
        },
        _toggleSet(set, item) {
            const newSet = new Set(set);
            if (newSet.has(item)) {
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
            try {
                await navigator.clipboard.writeText(xml);
            } catch (err) {
                // Fallback for older browsers
                this._fallbackCopy(xml);
            }
        },
        _fallbackCopy(text) {
            const textArea = document.createElement('textarea');
            textArea.value = text;
            textArea.style.position = 'fixed';
            textArea.style.opacity = '0';
            document.body.appendChild(textArea);
            textArea.select();
            try {
                document.execCommand('copy');
            } catch (err) {
                console.error('Copy failed:', err);
            }
            document.body.removeChild(textArea);
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
            this.typeFilter = 'all';
            this.extensionFilter = 'all';

            // Collapse all cards and expand only the target definition
            this.expandedDefs.clear();
            this.expandedDefs.add(definitionId);
            this.expandedDefs = new Set(this.expandedDefs);

            // Clear XML display state
            this.showXML.clear();
            this.showXML = new Set(this.showXML);

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
    },
    beforeUnmount() {
        window.removeEventListener('hashchange', this.handleHashChange);
    }
}).mount('#app');
