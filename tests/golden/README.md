# Golden references

Reference renders made by the C++ version of Project Iris (commit 286dd52), used by
`crates/iris-cli/tests/golden.rs` to check that the Rust engine renders the same images.

- `*.iris.json` — the edits for each reference (`neutral.png` has none).
- `info.txt` — `iris-cli --info` output for the RAW file, so the test can tell it was given
  the right one.
- `cpp-written.iris.json`, `cpp-preset.json` — a sidecar and a preset written by the C++
  version, read by the `iris-persist` tests.

The reference images themselves (`*.png`, `*.tif`) are renders of a private photo and are
not in git. The test is skipped unless they exist and `IRIS_TEST_RAW` points at that photo:

```sh
IRIS_TEST_RAW=/path/to/20260927_0001.ARW cargo test -p iris-cli --test golden
```

To make references for another RAW file, build the C++ version at commit 286dd52 and run
its `iris-cli` with `--long-edge 1200` for each sidecar (and `--long-edge 600 --16bit` for
`basic16.tif`), then replace `info.txt` with its `--info` output.
