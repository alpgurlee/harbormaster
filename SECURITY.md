# Security

## Reporting a vulnerability

Please report security problems privately through
[Report a vulnerability](https://github.com/alpgurlee/harbormaster/security/advisories/new) on the
Security tab, not in a public issue. Say which commit or release you ran, your macOS version, and the
steps that show the problem.

## What Harbormaster can do

Harbormaster runs as you, with no administrator rights and no helper tool, and it won't run as root:
its checks judge what is safe to stop or remove by your own user. It makes no network connections of
its own: there is no telemetry and no update check. Opening a server in the browser hands its address
to your browser.

It can end processes and delete files, so both go through checks first:

- A stop lists the processes it will signal first. Right before each signal it checks again that the
  process is the one you approved: the same PID and start time, and no other program started since.
  Where macOS can't tell the last of these, it stops nothing. It refuses system processes, other
  users' processes, the databases it recognizes, processes it couldn't fully read, anything
  Harbormaster itself runs under, and a server whose socket your shell, terminal or coding agent holds
  too. A forced stop is offered only after a polite one, and sent only if you choose it.
- The cleaner removes only items it listed and you chose. Right before each, it checks that the item is
  still the one the scan found and that none of your processes it can read is using it. A folder goes
  with whatever it holds by then, files added since the scan included. What may be yours to keep is
  never chosen for you and goes to the Trash; everything else, the Trash's contents included, is
  deleted for good.

## Known limits

- A check and the action it guards are separate steps. Between the last check and a signal, a process
  could start another program, or exit and have its PID reused, and the signal would reach a program
  you didn't approve; between the check and a removal, something could start using the item. The
  checks narrow these windows but don't close them. On Windows, a stop holds each process open from
  the check of who it is until it has ended, so no other process can take its PID in between.
- Command lines are shown with likely secrets hidden: the value of every `NAME=value` word; the value
  of a flag or header whose name mentions a credential, such as a token, secret, password, auth, cookie,
  or API or access key; a URL's user info, and query or fragment values under such names or named
  `key`, `sig`, `signature`, `jwt` or `sid`; and the word after `Bearer`. Values of other flags, such as
  `--port=3000`, stay visible apart from any URL secrets in them, since hiding them all would blank
  most dev commands.
- A process that rewrites its own arguments can make parts of its environment read as arguments. Those
  are hidden as `NAME=value` words are, except an entry whose name starts with `-`, which reads as a
  flag.
