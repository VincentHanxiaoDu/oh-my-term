# Complete the web client's surfaces

## Why

The browser client calls **16 of the instance's 140 capabilities** and has
**8 screens**. Everything else the architecture specifies for it — the file
tree, diffs, attachments, worktrees, settings — is documented, has a
capability behind it, has a typed client function generated for it, and has no
route a person can reach.

The parity gate did not catch this and is not wrong: [03 §5](../../../docs/architecture/03-capability-catalog.md)
requires a *web handler*, and there is one for all 140. A handler is a typed
call, not a screen. The gate proves the client can express every capability;
nothing proved somebody could get to one.

What this costs, concretely:

- **A phone cannot start work.** Every screen assumes a session exists. Opening
  a project needs a laptop, so the client is a viewer — and the moment somebody
  wants to act on what they came to check, they have to go and find one.
- **An agent session cannot be given a file or an image**, which is the input
  people reach for most on a phone: a screenshot of the thing that is broken.
  `media.attach`, `media.put` and `media.clipboard` all exist and only
  `fs.write` is called.
- **No diff.** `git.status`, `git.diff` and `git.hunks` are declared, doc 15
  §7.3 and §7.4 specify the mobile sheet and how a diff reads on a phone, and
  neither exists. Reviewing what an agent changed is the second reason to open
  this client and there is no way to do it.
- **No settings.** `config.schema` answers "every setting, whether or not
  anyone set it" and nothing renders it, so the client is configurable only by
  editing a file on the machine it is a remote for.

Two references were surveyed for what a browser client for this kind of tool
offers in practice. Both carry considerably more function than omt's client and
neither is usable one-handed; the useful output was a feature inventory, not a
design. Everything worth taking from it is already in omt's documents. Three
things were not, and are added by this change to
[08 §9.5–9.7](../../../docs/architecture/08-web-client.md):

- starting work from a client with no filesystem,
- where cost and context appear, and
- language.

## What changes

The client gains the surfaces its own documents specify, and the three new
sections above. In priority order, which is the order somebody hits them:

1. **Open a project, and start a session in it.** From the workspaces the
   instance already knows — no path field, because a phone cannot check a path
   before typing it.
2. **Attachments.** A photo, a file, or the clipboard, into an agent session's
   composer, through the pipeline [09 §4.3](../../../docs/architecture/09-ssh-and-media.md)
   already owns.
3. **Files and diffs.** The explorer's mobile sheet and the diff reader from
   doc 15 §7.3–7.4.
4. **Worktrees**, which doc 15 §5.3 already models and which is how one repo
   becomes several sessions.
5. **Settings**, from `config.schema`.
6. **Cost and context** on a session's own screen.
7. **Language**, English and Simplified Chinese.

## What does not change

- **No new capability is declared for any of it.** Every surface here is a
  route to something the instance already answers. A capability added to serve
  one screen is [19 §2.1](../../../docs/architecture/19-onboarding.md)'s
  "catalog entry that exists to be a screenshot".
- **No VCS mutation.** Doc 15 §1.1 decided against stage/unstage in v1 and this
  does not reopen it: the client reads diffs and does not write them.
- **Creating a workspace at an arbitrary path stays a laptop operation.** A
  phone that could would need a file browser rooted at `/`, which is a security
  surface bought for a case `omt` in a terminal already handles in one word.
- **The parity gate keeps its current meaning.** Making it assert reachability
  rather than expressibility is a real question and a separate change; this one
  would pass either way once it is done.
