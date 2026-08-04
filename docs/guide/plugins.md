# Writing a plugin

A plugin is third-party code running beside somebody's terminal. Everything
about the interface follows from that: permissions are declared up front, shown
before install, and enforced at every call.

## A minimal plugin

```
~/.config/omt/plugins/notify-ntfy/
├── omt-plugin.json     # the manifest
└── plugin.js           # the code
```

```json
{
  "id": "notify-ntfy",
  "name": "ntfy notifications",
  "version": "1.0.0",
  "description": "Push a notification when an agent needs you",
  "permissions": ["read_sessions", "network"]
}
```

The `id` becomes a config namespace and a directory name, so it is lowercase
letters, digits and hyphens. Anything that could escape either is refused at
install.

A plugin that declares **no** permissions is refused. That is either a mistake
or an attempt to be granted things later without having been shown.

## Permissions

Six, deliberately coarse. A permission a user cannot picture is one they click
through, and a list of forty is worse than a list of six.

| Permission | Shown to the user as | High consequence |
|---|---|---|
| `read_sessions` | see what is on your terminals | |
| `write_input` | type into your terminals | ⚠️ |
| `read_workspace` | read files in your projects | |
| `write_workspace` | change files in your projects | ⚠️ |
| `network` | send data over the network | ⚠️ |
| `spawn_process` | run programs on this machine | ⚠️ |

The middle column is what the install dialog shows. Descriptions are phrased as
what the plugin can *do*, not as the name of a flag — somebody granting
`write_input` needs to read "type into your terminals".

The four marked ⚠️ can make something happen the user did not do, or move their
data off the machine. A surface should stop on those.

## What a plugin may call

```js
// Every call names a permission. The host checks it; the call does not.
const entries = await omt.fs.list({ workspace, path: "src" });
const text    = await omt.fs.read({ workspace, path: "src/main.rs" });
await omt.fs.write({ workspace, path: "notes.md", contents });

const screen  = await omt.session.read({ session });
await omt.session.write({ session, text: "\n" });

await omt.notify({ message: "build finished", level: "info" });
const body    = await omt.http.get({ url });
```

### Paths

**A plugin never sees or sends an absolute path.** It names a file relative to
a workspace and the host resolves it. That is what keeps "read files in your
projects" a true description of the grant rather than an optimistic one.

Paths are checked twice, and both checks are needed:

1. **In the host, on the string, before any filesystem call.** Catches `..` and
   absolute paths without a syscall — so a plugin cannot use rejection timing
   to learn what exists outside its workspace.
2. **In the workspace layer, against the canonical path.** Catches symlinks,
   which the string check cannot see. A link inside a workspace pointing at `/`
   is the escape people forget.

Writes are capped at 8 MiB per call. A plugin is third-party code with a
file-writing permission, and "it only writes small files" is not something the
host can know.

## Updates

An update that asks for more than was granted does **not** get it. The host
compares the new manifest against what the user actually agreed to and reports
the difference:

```
`notify-ntfy` 1.1.0 additionally wants to:
  ⚠️ type into your terminals
```

Until that is granted, the plugin runs with what it had. Version 1.1 quietly
gaining `write_input` is the supply-chain shape this exists to catch.

Grants are also intersected with declarations, so a hand-edited config cannot
produce a capability the user was never shown.

## Testing a plugin

```sh
omt plugin link ./my-plugin      # run from a directory, no install
omt plugin list                  # what is installed and what it may do
omt plugin log my-plugin         # what it has been doing
omt plugin disable my-plugin     # off without uninstalling
```

`link` is the development path: the plugin runs from where you are editing it,
with the same permission checks as an installed one. A development mode that
skipped the checks would let you ship something that only works unchecked.

## What a plugin cannot do

- Reach a file outside a workspace it was granted
- Type into a session without `write_input`
- Escalate by updating
- Read another plugin's data
- Prevent the user from disabling it

If you need something that is not on the list, that is a conversation about a
new permission — not a workaround. A permission that gets worked around is one
that was never really enforced.
