# Property tests

Run `bun run test:props` for the property suite, runner checks, and convention checks.
The entrypoint imports every property module; the convention check enforces that
all property assertions use `assertProperty` with a unique literal identifier.

The runner first replays the committed entries in `property-seeds.json`, then
generates 200 cases with a fixed seed (individual properties can choose a smaller
budget). Public CI and ordinary local runs use the same seed. The run factor
scales both generated cases and the suite timeout; pinned replays always run once.

To replay a generated failure, select its test by name and use the seed and path
printed by fast-check:

```sh
PROPERTY_TEST_SEED=-530956778 PROPERTY_TEST_PATH=740:1:3:1:1:1:1:5:3:5:3:3:7:10:0:11:13:4:9:9 \
  bun test __test__/properties.spec.ts -t 'wholeWords returns valid'
```

To widen a deterministic run, set `PROPERTY_TEST_NUM_RUNS_FACTOR=10`. Setting
`PROPERTY_TEST_SEED` explicitly selects another reproducible run; the factor
alone does not choose a random seed. Add retained replays under their property id
with `{ "seed", "path", "note", "date" }`; notes are neutral and dates use
`YYYY-MM-DD`. The convention check validates the registry against the call sites.

Whole-word matching filters candidates before greedy selection. Its oracle checks
candidate distances, boundaries, and non-overlap; it does not compare the selected
results with unrestricted selected results. Restricted Damerau uses Optimal
String Alignment, so only Levenshtein is checked for the triangle inequality.
