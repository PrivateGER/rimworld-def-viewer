const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const { test } = require('node:test');

const appSource = fs.readFileSync(require.resolve('./app.js'), 'utf8');

function createState(categories = defaultCategories()) {
    let component;
    const sandbox = {
        clearTimeout,
        console,
        setTimeout,
        TextDecoder,
        Vue: {
            createApp(options) {
                component = options;
                return { mount() {} };
            }
        }
    };
    vm.runInNewContext(appSource, sandbox, { filename: 'app.js' });

    const state = component.data();
    for (const [name, method] of Object.entries(component.methods)) {
        state[name] = method.bind(state);
    }
    for (const [name, getter] of Object.entries(component.computed)) {
        Object.defineProperty(state, name, { get: getter.bind(state) });
    }

    state.$refs = {};
    state.$nextTick = callback => callback();
    state.categories = categories;
    state.stats = { total_defs: categories.reduce((total, category) => total + category.definitions.length, 0) };
    state.defsById = {};
    for (const category of categories) {
        for (const definition of category.definitions) {
            state.defsById[definition.id] = { def: definition, category: category.name };
        }
    }
    state.rebuildDefinitionIndex();
    return state;
}

function defaultCategories() {
    return [
        {
            name: 'ThingDef',
            definitions: [definition('steel', 'Steel', 'ThingDef')]
        },
        {
            name: 'RecipeDef',
            definitions: [definition('medicine', 'MakeMedicine', 'RecipeDef')]
        }
    ];
}

function definition(id, defName, defType, overrides = {}) {
    return {
        id,
        def_name: defName,
        def_type: defType,
        extension: 'Core',
        file_path: `Data/Core/Defs/${id}.xml`,
        is_abstract: false,
        tags: [],
        references_out: [],
        references_in: [],
        code_references: [],
        ...overrides
    };
}

test('one- and two-character searches render no results', () => {
    const state = createState();

    state.searchInput = 'st';
    state.applySearch();

    assert.equal(state.activeCategory, 'all');
    assert.equal(state.isShortSearch, true);
    assert.equal(state.searchCharactersNeeded, 1);
    assert.equal(state.searchResults.length, 0);
    assert.equal(state.visibleSearchResults.length, 0);
});

test('search input is applied after the 150 ms debounce', async () => {
    const state = createState();

    state.searchInput = 'steel';
    state.performSearch();
    assert.equal(state.searchQuery, '');

    await new Promise(resolve => setTimeout(resolve, 175));
    assert.equal(state.searchQuery, 'steel');
    assert.equal(state.activeCategory, 'all');
    assert.deepEqual(Array.from(state.searchResults, result => result.id), ['steel']);
});

test('global search ranks names before descriptive matches', () => {
    const state = createState([{
        name: 'ThingDef',
        definitions: [
            definition('description', 'MetalChair', 'ThingDef', { description: 'Built from steel' }),
            definition('substring', 'Plasteel', 'ThingDef'),
            definition('prefix', 'SteelWall', 'ThingDef'),
            definition('exact', 'Steel', 'ThingDef')
        ]
    }]);

    state.searchInput = 'steel';
    state.applySearch();

    assert.deepEqual(
        Array.from(state.searchResults, result => result.id),
        ['exact', 'prefix', 'substring', 'description']
    );
});

test('search results are progressively disclosed in batches of 50', () => {
    const definitions = Array.from({ length: 120 }, (_, index) =>
        definition(`item-${index}`, `Item${index}`, 'ThingDef')
    );
    const state = createState([{ name: 'ThingDef', definitions }]);

    state.searchInput = 'item';
    state.applySearch();

    assert.equal(state.searchResults.length, 120);
    assert.equal(state.visibleSearchResults.length, 50);
    assert.equal(state.hasMoreSearchResults, true);

    state.showMoreSearchResults();
    assert.equal(state.visibleSearchResults.length, 100);
});

test('type and extension filters use the same cached result set', () => {
    const state = createState([{
        name: 'ThingDef',
        definitions: [
            definition('core', 'SteelCore', 'ThingDef'),
            definition('abstract', 'SteelAbstract', 'ThingDef', { is_abstract: true }),
            definition('royalty', 'SteelRoyalty', 'ThingDef', { extension: 'Royalty' })
        ]
    }]);
    state.searchInput = 'steel';
    state.applySearch();

    state.setTypeFilter('concrete');
    state.setExtensionFilter('Royalty');

    assert.deepEqual(Array.from(state.searchResults, result => result.id), ['royalty']);
    assert.equal(state.filteredDefinitionsCount, 1);
    assert.deepEqual(Array.from(state.visibleCategories, category => category.name), ['ThingDef']);
});

test('clearing the final active filter returns global results to overview', () => {
    const state = createState();
    state.searchInput = 'steel';
    state.applySearch();
    assert.equal(state.activeCategory, 'all');

    state.clearSearch();

    assert.equal(state.activeCategory, 'overview');
    assert.deepEqual(Array.from(state.displayedCategories), []);
});

test('opening cached XML and closing the drawer restores trigger focus', async () => {
    const state = createState();
    let triggerFocused = false;
    let closeFocused = false;
    const trigger = { focus() { triggerFocused = true; } };
    state.$refs.xmlCloseButton = { focus() { closeFocused = true; } };
    state.rawXmlById = { steel: '<ThingDef />' };

    await state.openXML('steel', { currentTarget: trigger });

    assert.equal(state.xmlDefinitionId, 'steel');
    assert.equal(closeFocused, true);
    assert.equal(state.rawXmlLoadingIds.has('steel'), false);

    state.closeXML();
    assert.equal(state.xmlDefinitionId, null);
    assert.equal(triggerFocused, true);
});

test('an XML lookup failure stays local to the initiating result', async () => {
    const state = createState();
    state.rawXmlById = {};

    await state.openXML('steel', { currentTarget: { focus() {} } });

    assert.equal(state.xmlDefinitionId, null);
    assert.equal(state.rawXmlLoadError.definitionId, 'steel');
    assert.match(state.rawXmlLoadError.message, /Raw XML not found/);
});

test('Escape clears search when the XML drawer is closed', () => {
    const state = createState();
    let prevented = false;
    let searchFocused = false;
    state.$refs.searchInput = { focus() { searchFocused = true; } };
    state.searchInput = 'steel';
    state.applySearch();

    state.handleGlobalKeydown({
        key: 'Escape',
        target: { tagName: 'BODY' },
        preventDefault() { prevented = true; }
    });

    assert.equal(prevented, true);
    assert.equal(searchFocused, true);
    assert.equal(state.searchQuery, '');
    assert.equal(state.activeCategory, 'overview');
});
