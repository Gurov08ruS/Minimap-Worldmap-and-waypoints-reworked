These are the private-renderer fixtures last executed against Horizon `9f42064`
with the old engine-rebuild route (18 map/UI cases and nine binding UI cases).
They are retained as regression specifications, not counted as SDK test passes.
The modern SDK supplies renderer metadata, rather than a linkable renderer test
binary. Current checks compile the real adapters against the signed prebuilt
engine, run the shared runtime tests both standalone and in the SDK crate, audit
the actual Mixins output, and exercise the installed package in a disposable game
instance. `examples/sdk_fixture.rs` supplies two Cyrillic markers for that run.
The SDK owns the Controls and range widgets; the old renderer widget overrides
are no longer shipped.
