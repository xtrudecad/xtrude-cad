# Releasing Xtrude CAD

For the maintainer. A release is a version tag, `vX.Y.Z`, pushed to the
public repository `xtrudecad/xtrude-cad`. GitHub Actions
(`.github/workflows/release.yml`) does the rest: it runs the full checks,
builds the release files and makes a **draft** GitHub release of them.
Nothing is built by hand; the maintainer reviews the draft and publishes
it.

## 1. Prepare the release commit

1. Bump the workspace `version` in `Cargo.toml` (the single source:
   `xtrude --version`, the `.igz` `app_version`, the tarball's name) and
   let Cargo update `Cargo.lock` (`cargo check`; CI builds with
   `--locked`).
2. Add the release to the top of `<releases>` in
   `packaging/linux/org.xtrudecad.XtrudeCAD.metainfo.xml` (AppStream
   lists releases newest first), dated today, with a `<description>`:

   ```xml
   <release version="X.Y.Z" date="YYYY-MM-DD">
     <description>
       <p>What changed, for users.</p>
       <ul>
         <li>One change per item; <code>code</code> and <em>emphasis</em> are allowed.</li>
       </ul>
     </description>
   </release>
   ```

   That description becomes the draft's release notes, converted to
   Markdown by `scripts/release/release_notes.py`. A release without a
   description gets GitHub's generated notes instead. Software centres
   show the same text.
3. Point the screenshot URLs in the metainfo (`<screenshot>`, `…/vX.Y.Z/
   packaging/screenshots/…`) at the new tag. They are fetched from the
   tagged tree, so new or changed screenshots in `packaging/screenshots/`
   go into this commit too.
4. Run `scripts/check.sh`: `test_packaging.rs` checks that the newest
   metainfo release is the workspace version, and that every screenshot
   URL names a file in `packaging/screenshots/` at the tag `vX.Y.Z` (the
   version without its pre-release part, see
   [Release candidates](#release-candidates)), so a bump that forgets
   the URLs fails.
5. Commit, for example `release: X.Y.Z`, and push it to `main`.

## 2. Tag and push the tag

```bash
git tag -a vX.Y.Z -m "Xtrude CAD X.Y.Z"
git push origin vX.Y.Z
```

The tag goes to the **public** repository, `xtrudecad/xtrude-cad` (the
remote is `origin` in a clone of it): the workflow publishes to the
repository it runs in, and Flathub builds from public, tagged source. The
tag must name the workspace version exactly (`v0.1.0` for 0.1.0); the
workflow's first job (`scripts/release/check_tag.sh`) fails otherwise,
before anything is built.

## 3. Wait for the workflow

`release.yml` runs these jobs (Actions tab, workflow "release"):

- **verify**: the tag names the workspace version;
- **checks**: `scripts/check.sh`, as on every push;
- **tarball**: builds `xtrude-cad-X.Y.Z-x86_64-linux.tar.gz` in an
  `almalinux:8` container (glibc 2.28), checks both binaries against
  the 2.28 glibc floor (`scripts/release/check_glibc.sh`), runs the
  packed binary's `--version`, and installs and uninstalls the tarball
  in a temporary prefix with its `install.sh` (`scripts/check.sh`
  already tests the glibc check and `install.sh` on every push);
- **flatpak**: builds `xtrude-cad-X.Y.Z-x86_64.flatpak` and lints it
  with Flathub's linter (see [Flatpak](#flatpak));
- **release**: writes `SHA256SUMS`, attests every file's build
  provenance, and creates the draft release `Xtrude CAD X.Y.Z` with the
  files attached; a tag with a pre-release part (`vX.Y.Z-rc.N`) makes
  a pre-release draft (`scripts/release/prerelease.sh`, see
  [Release candidates](#release-candidates)). GitHub makes attestations only for a public
  repository on the organisation's plan: a tag pushed to a private one
  (a dry run) gets its draft without them, and a warning.

If a job fails, nothing is published. Fix the cause on `main`, then move
the tag to the fixed commit (no one has used it yet; delete a draft the
release job made first, with `gh release delete vX.Y.Z`):

```bash
git push origin :refs/tags/vX.Y.Z
git tag -fa vX.Y.Z -m "Xtrude CAD X.Y.Z"
git push origin vX.Y.Z
```

## 4. Review and publish the draft

On the releases page, open the draft and check:

- the notes (edit them here if needed; the metainfo keeps the original);
- the files: the tarball, the `.flatpak` bundle and `SHA256SUMS`;
- optionally, download the tarball and try it (`./install.sh`, then
  `./install.sh --uninstall`).

Then click **Publish release**. Publishing opens the Flathub update pull
request (see [Flathub](#flathub)). A published release is public and its
tag should never move again; a mistake found afterwards is fixed by the
next version.

## Release candidates

A release candidate is a test build for users before `X.Y.Z`: the same
steps, with a SemVer pre-release version, `X.Y.Z-rc.N` (`rc.1`, `rc.2`…):

1. The workspace `version` in `Cargo.toml` is `X.Y.Z-rc.N`
   (`xtrude --version` says `Xtrude CAD X.Y.Z-rc.N`).
2. Its metainfo entry has the same version and `type="development"`,
   which software centres do not offer as a stable release:

   ```xml
   <release version="X.Y.Z-rc.N" date="YYYY-MM-DD" type="development">
   ```

3. The screenshot URLs name the **final** tag, `vX.Y.Z`, not the rc's:
   they are the ones the final release ships. They resolve only once
   `vX.Y.Z` is tagged, so for an rc the Flatpak job warns that they do
   not resolve and accepts their two lint errors; for the final tag it
   fails instead.
4. Tag `vX.Y.Z-rc.N` and push it (step 2). The tag names the version,
   suffix included (`check_tag.sh`).

The release job sees the pre-release part of the tag and makes the
draft a **pre-release**. Publishing it leaves the "Set as a pre-release"
box ticked: GitHub does not show it as the latest release, and
`flathub-update.yml`, which skips pre-releases, never sends it to
Flathub. For the final release, set the version back to `X.Y.Z` and add
its own metainfo entry (`type="stable"`) above the rc's.

## Checking a release file

Every release file has a build attestation: a signed statement that the
release workflow built it from this repository, at the tag's commit. A
user checks it with the GitHub CLI:

```bash
gh attestation verify xtrude-cad-X.Y.Z-x86_64-linux.tar.gz --repo xtrudecad/xtrude-cad
```

and checks the download itself against the checksums:

```bash
sha256sum -c --ignore-missing SHA256SUMS
```

## Building the tarball locally

Not needed for a release, but useful to debug the tarball job. The
release build runs in the same container with podman (or docker); the
separate target directory keeps the container's objects apart from the
host's. Keep it on disk, not on a tmpfs `/tmp`: the first build of
OpenCascade takes about 20 minutes. This is the command in
`scripts/release/build_tarball.sh`'s header:

```bash
podman run --rm -v "$PWD":/src:z -w /src \
    -e CARGO_TARGET_DIR=/src/target/almalinux8 \
    -e CARGO_HOME=/src/target/almalinux8-cargo \
    -e RUSTUP_HOME=/src/target/almalinux8-rustup \
    almalinux:8 bash -c '
      dnf install -y -q gcc-toolset-14-gcc gcc-toolset-14-gcc-c++ \
          make cmake perl git binutils tar gzip &&
      source /opt/rh/gcc-toolset-14/enable &&
      curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
          --default-toolchain none --no-modify-path &&
      export PATH="$CARGO_HOME/bin:$PATH" &&
      scripts/release/build_tarball.sh &&
      scripts/release/check_glibc.sh target/almalinux8/release/xtrude 2.28 &&
      scripts/release/check_glibc.sh target/almalinux8/release/xtrude-step 2.28'
```

The tarball lands in `target/release-dist/`. AlmaLinux 8's own GCC 8
cannot build Manifold (the solid booleans), so the build uses
gcc-toolset-14's compiler, which targets the same glibc. rustup installs
the toolchain that `rust-toolchain.toml` pins.

## Flatpak

The manifest is `packaging/flatpak/org.xtrudecad.XtrudeCAD.yml`, the
same one Flathub gets, except that the in-tree copy builds the checkout
(a `dir` source) where Flathub's fetches the release tag.

### The Flatpak job

`.github/workflows/flatpak.yml` builds it in Flathub's container image
for the manifest's runtime branch
(`ghcr.io/flathub-infra/flatpak-github-actions:freedesktop-<branch>`;
`test_packaging.rs` checks the two agree, so a runtime bump updates
both), with `flatpak/flatpak-github-actions/flatpak-builder`. The
build is offline and compiles Manifold and OpenCascade from source,
without a cache, so it takes about an hour, most of it in OpenCascade
(the log stays on `Compiling xtrude-io` meanwhile: `occt-sys`' build
script prints nothing until it is done). It needs more disk than a
runner has free, so the job first deletes the runner's preinstalled
toolchains it never uses. Then it runs Flathub's linter, `flatpak-builder-lint`,
on the manifest and on the built repository (app id, metainfo, icons,
permissions), with the exceptions in
`packaging/flatpak/lint-exceptions.json`: the two permissions the
design needs, whose reasons are in that file and which Flathub grants
at submission. It uploads the bundle as the workflow artifact
`flatpak`.

The metainfo's screenshots link the public repository at the release
tag, which only exists once the release is tagged there.
`scripts/release/flatpak_lint_exceptions.py` checks their URLs first:

- when they resolve, the build mirrors them as Flathub's builder does
  (`--mirror-screenshots-url=https://dl.flathub.org/media`, committed to
  the repository), and the lint checks them;
- when they do not, the job warns and the lint accepts the two errors
  that follow (`appstream-missing-screenshots`,
  `appstream-screenshots-not-mirrored-in-ostree`). That is the normal
  case for `ci.yml` runs before the release and for a pre-release tag;
  for a **final** release (`vX.Y.Z` without a pre-release part) it is an
  error, and the job fails.

`release.yml` runs it for every tag; the **release** job waits for it, and the bundle is attached to the draft
with the other files (in `SHA256SUMS`, with an attestation). `ci.yml`
runs it only when `packaging/` or `flatpak.yml` changed
and on a manual run (Actions → ci → Run workflow), to spare the runner
minutes.

### Installing the bundle by hand

The bundle holds the app, not its runtime, which `flatpak` fetches from
Flathub:

```bash
flatpak remote-add --user --if-not-exists flathub \
    https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user xtrude-cad-X.Y.Z-x86_64.flatpak
flatpak run org.xtrudecad.XtrudeCAD
```

Its settings, crash logs and extensions are under
`~/.var/app/org.xtrudecad.XtrudeCAD/`, not `~/.config/xtrude`.
`flatpak uninstall --user org.xtrudecad.XtrudeCAD` removes it.

### Updating the cargo sources

The build cannot download crates: `packaging/flatpak/cargo-sources.json`
lists every crate of `Cargo.lock`, and flatpak-builder fetches them
beforehand. After every change to `Cargo.lock` (a dependency added,
removed or bumped), regenerate it and commit it with the change:

```bash
scripts/release/update_cargo_sources.sh
```

`ci.yml`'s `cargo-sources` job reruns the script and fails when the
committed file is out of date, and `test_packaging.rs` checks it
against `Cargo.lock`.

## Flathub

Not published yet. Once the app is on Flathub, its manifest lives in
the repository `flathub/org.xtrudecad.XtrudeCAD`: the in-tree manifest
with the xtrude-cad module's `dir` source replaced by the public
repository at a release tag (`type: git`, `url`, `tag`, `commit`), and
the tag's `cargo-sources.json` beside it.

### On publish: the update pull request

Publishing a release (not a pre-release) runs
`.github/workflows/flathub-update.yml`. It checks out the Flathub
repository, sets the xtrude-cad source's `tag` to the release tag and
its `commit` to the commit the tag points to (no other line of the
manifest changes), copies `packaging/flatpak/cargo-sources.json` from
the tag, pushes the branch `update-X.Y.Z` and opens the pull request
"Update to X.Y.Z", which links the GitHub release. A draft is never
published, so it never reaches Flathub.

Flathub's bot then builds the pull request. Once that test build passes,
the maintainer merges it, and Flathub publishes the update.

### The `FLATHUB_TOKEN` secret

The workflow pushes and opens the pull request with the
`FLATHUB_TOKEN` secret of `xtrudecad/xtrude-cad`: a fine-grained GitHub
token with **contents** and **pull-request** write access to
`flathub/org.xtrudecad.XtrudeCAD` only. Without the secret (before the
app is on Flathub) the workflow ends green with a notice saying it
skipped the update. The token expires: renew it before it does, or the
next release's workflow fails.

## The public repository's settings

Set once on `xtrudecad/xtrude-cad`, before the first release (they are
not in the repository, so nothing checks them):

- **Discussions** on (Settings → General → Features), with the
  categories Q&A, Ideas and Show and tell. The issue forms'
  `contact_links` (`.github/ISSUE_TEMPLATE/config.yml`), the README and
  the website send questions there.
- **Private vulnerability reporting** on (Settings → Code security),
  which `SECURITY.md` and the issue forms point to. GitHub offers it on
  public repositories only.
- The addresses the project publishes all reach the maintainer's inbox:
  hello@xtrudecad.org (general, legal, the metainfo's `update_contact`),
  security@xtrudecad.org (`SECURITY.md`) and conduct@xtrudecad.org
  (`CODE_OF_CONDUCT.md`).
