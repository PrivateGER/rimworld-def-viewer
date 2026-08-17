const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const { test } = require('node:test');

const appSource = fs.readFileSync(require.resolve('./app.js'), 'utf8');

function createState() {
    let component;
    const sandbox = {
        console,
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

    state.categories = [
        {
            name: 'ThingDef',
            definitions: [definition('steel', 'Steel', 'ThingDef')]
        },
        {
            name: 'RecipeDef',
            definitions: [definition('medicine', 'MakeMedicine', 'RecipeDef')]
        }
    ];
    state.stats = { total_defs: 2 };
    return state;
}

function definition(id, defName, defType) {
    return {
        id,
        def_name: defName,
        def_type: defType,
        extension: 'Core',
        is_abstract: false,
        tags: [],
        references_out: [],
        references_in: [],
        code_references: []
    };
}

test('searching from overview renders matching categories globally', () => {
    const state = createState();

    state.searchQuery = 'medicine';
    state.performSearch();

    assert.equal(state.activeCategory, 'all');
    assert.deepEqual(
        Array.from(state.displayedCategories, category => category.name),
        ['RecipeDef']
    );
    assert.equal(state.filteredDefinitionsCount, 1);
});

test('an inactive category is not included in displayed categories', () => {
    const state = createState();

    state.setActiveCategory('ThingDef');

    assert.deepEqual(
        Array.from(state.displayedCategories, category => category.name),
        ['ThingDef']
    );
});

test('clearing filters returns global results to overview', () => {
    const state = createState();
    state.searchQuery = 'steel';
    state.performSearch();
    assert.equal(state.activeCategory, 'all');

    state.searchQuery = '';
    state.performSearch();

    assert.equal(state.activeCategory, 'overview');
    assert.deepEqual(Array.from(state.displayedCategories), []);
});
