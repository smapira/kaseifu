# Tap-side agent handoff: `routeflags/homebrew-tap` Formula for kaseifu

Last updated: 2026-10-16.

Purpose: the exact brief for the agent that works inside the
`routeflags/homebrew-tap` checkout. That repository is NOT attached to the
thread where this file lives, so attach it (or open a thread that has it)
before touching anything. Read-only inspection from outside is fine; editing
requires the attached checkout.

Background plan: `documents/plans/dev/20261016d-homebrew-support.md`
(kaseifu side). This file is the executable handoff, not a second plan.

The same brief is filed on the tap repository as
<https://github.com/routeflags/homebrew-tap/issues/1> (created 2026-10-16),
so the tap-side agent can work from the issue. Keep the two in step when the
version or the measured hash changes.

## Objective

Create `Formula/kaseifu.rb` in `routeflags/homebrew-tap` so that this works
on a Mac that meets the Xcode prerequisite (the final acceptance test, run
through GitHub after the push):

```bash
brew tap routeflags/tap
brew trust --formula routeflags/tap/kaseifu
brew install kaseifu
kaseifu --version
```

## Starting state (verified 2026-10-16)

- Tap repository: `github.com/routeflags/homebrew-tap` is **public and
  empty** (no refs at all, `main` not created), `origin` set. The first
  push therefore **creates the branch** rather than fast-forwarding. Keep
  internal notes out of it because it is public.
- kaseifu releases are annotated tags (`github.com/smapira/kaseifu`,
  currently up to `v0.12.0`). Use the newest `vX.Y.Z` tag at work time;
  do not guess the version, read it with `git ls-remote --tags`.
- kaseifu ships two binaries (`kaseifu`, `kaseifu-bench`) from one Cargo
  package. Its `build.rs` compiles `swift/kaseifu_fm_bridge.swift` via
  `xcrun --find swiftc`: a source build needs **Xcode itself (Command Line
  Tools do not work)**. Measured: the `@Generable` macro plugin
  `libFoundationModelsMacros.dylib` ships only inside Xcode.app, so a
  CLT-selected build fails (`cargo install --locked --path .`: EXIT=0 with
  Xcode 26.6, EXIT=101 with CLT 26.6).
- No GitHub Release artifacts exist; use the automatic source tarball
  `https://github.com/smapira/kaseifu/archive/refs/tags/vX.Y.Z.tar.gz`.
- Org mismatch is expected: formula lives under `routeflags`, upstream is
  `smapira/kaseifu`.

## Steps

1. Attach (or open the thread on) the tap checkout. Confirm
   `git status` is clean and `git log --oneline` shows the expected base.
2. Decide the exact version: newest `v*` tag of `smapira/kaseifu`.
3. Fetch the tarball with `curl -L` and record `shasum -a 256`. Never
   invent the hash.
4. Write `Formula/kaseifu.rb`:
   - `desc` one ASCII line; `homepage https://github.com/smapira/kaseifu`;
     `url` + measured `sha256`; `license "MIT"`.
   - `depends_on "rust" => :build`.
   - `depends_on xcode: ["26.0", :build]`. `XcodeRequirement` compares the
     **Xcode.app version**, not the SDK version (measured point: 26.6). Do
     not describe this as an SDK requirement, and do not downgrade to CLT:
     that path is measured to fail.
   - `install` runs the equivalent of `cargo install --locked --path .`
     (`--locked` keeps the build reproducible; the tarball ships
     `Cargo.lock`, so the flag works).
   - Install both binaries (`kaseifu`, `kaseifu-bench`).
   - Do not add `depends_on macos:`: the binary weak-links FoundationModels
     and still starts on macOS < 26.
   - No `livecheck` block: the default strategy for an archive URL is
     `Git`, which already follows the `vX.Y.Z` tags (`GithubLatest` cannot
     apply because there are zero GitHub Releases). If an explicit block is
     wanted, use `strategy :git`.
   - `test do` performs side-effect-free checks only (`--version`,
     no-target guidance exit code, `label --help` ASCII). No journal,
     Keychain, or real-file contact.
5. Make the tap visible to brew as a **pre-push smoke check** only, then
   verify by **name** (Homebrew 7 rejects path-based `audit` / `install` /
   `livecheck`):

   ```sh
   ln -s "$PWD" "$(brew --repository)/Library/Taps/routeflags/homebrew-tap"
   brew install --build-from-source routeflags/tap/kaseifu
   brew test routeflags/tap/kaseifu
   brew audit --strict --new routeflags/tap/kaseifu
   brew livecheck routeflags/tap/kaseifu
   ```

   Also confirm `kaseifu --version` matches the tag. Record exact commands
   and outputs. Installing changes the prefix (`rust`, `llvm`, `openssl@4`
   arrive; existing formulae may be upgraded): **confirm before running it**.
   If brew asks for trust, run `brew trust --formula
   routeflags/tap/kaseifu` first or keep the fully-qualified name.
6. Commit the Formula and push to the tap `origin` only (the first push
   creates `main`). Never rewrite history, never force-push.
7. Run the **GitHub-based acceptance test**, which doubles as the
   version-bump procedure check:

   ```sh
   brew tap routeflags/tap
   brew trust --formula routeflags/tap/kaseifu
   brew install kaseifu
   kaseifu --version
   ```

   Report the exact output. This step is the one the tool side records as the
   Phase 1 acceptance criterion. Then report back: version used, measured
   sha256, the smoke log, the acceptance log, and any Xcode/SDK finding.

## Verification status (tool side, 2026-10-16)

Measured here, so the tap agent can rely on it:

- v0.12.1 source tarball, `cargo install --locked --path . --root <tmp>`
  with Xcode 26.6 selected: **exit 0**, 58 s, both binaries installed,
  `--version` matches the tag, a bare run exits 0, `label --help` is ASCII,
  and an isolated HOME receives no files.
- The same command with `DEVELOPER_DIR=/Library/Developer/CommandLineTools`:
  **exit 101**, `plugin for module 'FoundationModelsMacros' not found` at
  `swift/kaseifu_fm_bridge.swift:478`. Hard failure, not the stub fallback.

Not verified here (do not claim them as done):

- `brew install --build-from-source` / `brew test` / name-based
  `brew audit` / `brew style` / `brew livecheck` (the pre-push smoke check).
- The GitHub-based acceptance test (needs the pushed formula).
- A machine that never had Xcode, and a clean machine without rust/llvm:
  tracked in `documents/plans/dev/issues/waiting-user/20261016-waiting-clean-mac-verification.md`.

### Tap side landed (2026-10-16)

The tap agent pushed the formula and reported on the tap issue. Tool-side
review of the pushed file (`Formula/kaseifu.rb` at
<https://github.com/routeflags/homebrew-tap/issues/1>):

- Commits: `a7981eb` (formula, v0.12.0) and `42a3566` (bump to v0.12.1).
- Content matches the reference: `url` v0.12.1, measured `sha256`, `license
  "MIT"`, `depends_on "rust" => :build`, `depends_on xcode: ["26.0",
  :build]`, no `livecheck` block, install through `std_cargo_args`.
- Verified from this side: `std_cargo_args` exists in this Homebrew
  (`formula.rb:2227`, expands to `--jobs N --locked --root=prefix
  --path=.`), and `kaseifu-bench --version` prints `kaseifu-bench 0.12.1`
  with exit 0, so the added test assertion holds. `ruby -c` on the formula
  is clean.

#### Homebrew-side checks run on this machine (2026-10-16, no prefix change)
```sh
brew tap routeflags/tap                 # Tapped 1 formula (cloned from GitHub)
brew audit --strict --new routeflags/tap/kaseifu   # exit 0, no findings
brew style routeflags/tap/kaseifu       # 1 file inspected, no offenses
brew livecheck routeflags/tap/kaseifu   # kaseifu: 0.12.1 ==> 0.12.1
```

So the tap is live, the formula is audited and styled clean, and livecheck
follows the tags. `brew tap` leaves a clone under
`$(brew --repository)/Library/Taps/routeflags/homebrew-tap`; `brew untap
routeflags/tap` removes it.

Still pending: the tap bump to v0.12.2 and then the CI acceptance run. The
first two CI runs found a real failure (the `SDKROOT` problem below), which
is exactly the point of running the acceptance path in isolation.

One measured side effect the tap test block had to work around (tool-side
issue filed): `kaseifu label` with no target creates the state database in
HOME even though it only prints guidance:
`documents/plans/dev/issues/open/20261016-open-label-guidance-touches-state.md`.

## Appendix: reference Formula text (reconcile with the draft that passed audit)

The draft in the tap worktree reportedly passes an in-process strict+new
online audit with 0 problems; that artifact is authoritative. This text is
the tool-side reference for the same formula, with the measured values
filled in, so the two cannot drift silently. Reconcile rather than replace.

```ruby
class Kaseifu < Formula
  desc "Semantic file organization for macOS"
  homepage "https://github.com/smapira/kaseifu"
  url "https://github.com/smapira/kaseifu/archive/refs/tags/v0.12.2.tar.gz"
  sha256 "70b76240f441913b3a1e85fc8f1edba964c2475bd1d42ddbf54ba95cad58dfd3"
  license "MIT"

  # A source build compiles swift/kaseifu_fm_bridge.swift, so Xcode itself is
  # required: the @Generable macro plugin ships only inside Xcode.app.
  depends_on "rust" => :build
  depends_on xcode: ["26.0", :build]

  def install
    system "cargo", "install", "--locked", "--path", ".", "--root", prefix
  end

  test do
    # No key, journal, completion or real file is touched: the isolated HOME
    # plus these toggles keep the test side-effect free.
    ENV["KASEIFU_KEY_SOURCE"] = "env"
    ENV["KASEIFU_NO_COMPLETION_UPDATE"] = "1"
    assert_match "kaseifu #{version}", shell_output("#{bin}/kaseifu --version")
    assert_match "usage:", shell_output("#{bin}/kaseifu")
    assert_match "usage:", shell_output("#{bin}/kaseifu label --help")
  end
end
```

Points the tool side verified about this text:

- Both binaries come from the one package, so `cargo install --path .`
  installs `kaseifu` and `kaseifu-bench` (measured).
- A bare `kaseifu` prints guidance and exits 0 (measured), so
  `shell_output(...)` without a status argument holds.
- `kaseifu label --help` is pure ASCII (measured), so it is safe in a test.
- No `livecheck` block: the default `Git` strategy follows the tags.
- No `depends_on macos:`: the binary weak-links FoundationModels and starts
  on macOS < 26.

## Constraints

- The tap's first push creates `main`; afterwards fast-forward only. No
  history rewrites on any shared branch.
- No secrets in the Formula or logs (no tokens, no API keys).
- Verification must not read or write a real user's files, Keychain, or
  journal; use an isolated HOME and temp dirs.
- Do not touch the kaseifu checkout from this assignment; kaseifu-side
  docs are maintained in the other thread.
- If the clean-machine Xcode prerequisite cannot be met, stop and report
  exactly what is missing instead of working around it.
- Measured reference values (2026-10-16): latest tag `v0.12.2`, tarball
  sha256 `70b76240f441913b3a1e85fc8f1edba964c2475bd1d42ddbf54ba95cad58dfd3`
  (v0.12.0 carried no `LICENSE`; v0.12.1 fails a Homebrew build because
  Homebrew's `SDKROOT` points at the Command Line Tools SDK, so use v0.12.2
  or newer), Homebrew 7.0.9 locally and 6.0.22 on the CI runner, Xcode 26.6.
  Re-measure the hash for the version you ship.
