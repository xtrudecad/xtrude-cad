# Security policy

## Supported versions

Xtrude CAD is in pre-release (0.x). Security fixes go into the latest
release only; please check that a problem is still there in it, or on
`main`, before reporting it.

## Reporting a vulnerability

Please do **not** open a public issue, discussion or pull request for a
security problem. Report it privately instead:

- on GitHub: **Security → Report a vulnerability** in the
  [repository](https://github.com/xtrudecad/xtrude-cad/security/advisories/new)
  (preferred: the report, the discussion and the fix stay together);
- or by email to **security@xtrudecad.org**.

Include what you can of:

- the Xtrude CAD version (`xtrude --version`) and your system;
- what an attacker can do, and what they need (a file the user opens, an
  extension the user installs…);
- the steps or a file that reproduce the problem.

Xtrude CAD is maintained by volunteers. You should get an
acknowledgement within a week. We will tell you what we find, agree with
you on when the problem is disclosed (usually once a fixed release is
out), and credit you in the advisory unless you prefer not to be named.

## Scope

Examples of what we treat as security problems:

- opening a document or an imported file (`.xtrude`, `.igz`, STL, OBJ, glTF, DAE,
  DXF, `.skp`, STEP) that runs code, writes or reads files it should
  not, or crashes the editor in a way an attacker controls;
- a Lua extension that gets out of its sandbox: reaches files, the
  network, processes or the environment beyond what the extension API
  allows, or escapes its memory and time limits;
- a release file whose checksum or build attestation does not match.

A file that makes Xtrude CAD slow or makes it refuse to open is a bug
rather than a vulnerability: report it as an
[issue](https://github.com/xtrudecad/xtrude-cad/issues).
