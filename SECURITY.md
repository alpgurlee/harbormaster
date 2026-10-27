# Security

## Reporting a vulnerability

Please report security problems privately through
[Report a vulnerability](https://github.com/alpgurlee/harbormaster/security/advisories/new) on the
Security tab, not in a public issue. Say which commit or release you ran, your macOS or Windows version,
and the steps that show the problem.

## What Harbormaster can do

Harbormaster runs as you, with no administrator rights and no helper tool, and it won't run as root:
its checks judge what is safe to stop or remove by your own user. On Windows it may ask Windows to start
an app's own uninstaller as an administrator, after you confirm and only once Windows' own prompt has
your yes; Harbormaster itself stays as it was. Opening a server in the browser hands its address to your
browser.

Its one network connection of its own is the update check. When it starts and every six hours after, unless
you turn the check off in Settings → About, it asks GitHub for the newest release's `latest.json`; GitHub sees
your IP address and the request, as any website does, and Harbormaster sends no account or device identifier.
Only when you choose Update does it download the new version, from GitHub, and it installs nothing whose
signature doesn't check against the key built into the app, nor a version older than the one running. There is
no telemetry.

It can end processes, delete files and, on Windows, start an app's uninstaller, so each goes through
checks first:

- A stop lists the processes it will signal first. Right before each signal it checks again that the
  process is the one you approved: the same PID and start time, and no other program started since.
  Where macOS can't tell the last of these, it stops nothing. It refuses system processes, other
  users' processes, the databases it recognizes, processes it couldn't fully read, anything
  Harbormaster itself runs under, and a server whose socket your shell, terminal or coding agent holds
  too. A forced stop is offered only after a polite one, and sent only if you choose it.
- The cleaner removes only items it listed and you chose. Right before each, it checks that the item is
  still the one the scan found and that none of your processes it can read is using it. A folder goes
  with whatever it holds by then, files added since the scan included. What may be yours to keep is
  never chosen for you and goes to the Trash, or on Windows the Recycle Bin; everything else, the
  Trash's contents included, is deleted for good. On Windows the move is the shell's, which deletes for
  good what the Recycle Bin can't take, such as an item larger than the bin allows. A file larger than
  the bin is never handed to it, and when the shell says it won't recycle an item, Harbormaster stops
  it before it deletes anything; either stays where it was. Should the shell delete an item for good
  all the same, the removal says so. The Recycle Bins are emptied by the shell too; their folders are
  never touched.
- On Windows the cleaner removes no app itself. It starts the app's own uninstaller as the app's
  uninstall entry in the registry names it, and only for an app it listed and you confirmed. Right
  before, it reads the entry again and starts nothing if the entry changed. Windows reads the command,
  and one whose program Windows can't be sure of, such as an unquoted path with spaces, isn't started.
  The uninstaller starts as you. Only when it asks for an administrator is Windows asked to start it as
  one, and only for an app installed for everyone whose uninstaller is kept in Program Files or the
  Windows folder, where only administrators can change it, and checked to be so: neither the
  uninstaller, the folders from it up to there, nor the files beside it may be changeable by your
  programs that run without an administrator. It then starts from there, in its own folder. An app
  installed for you alone is never started as an administrator, since any program of yours can change
  its entry. Whether the app is then removed is the uninstaller's to say, and Harbormaster doesn't
  check.
- The cleaner lists, as data an app removed from here may have left, only entries that carry the name of
  an app removed from Harbormaster's Apps section in this run. On a Mac that is an app moved to the Trash
  from here, whose bundle identifier is read from its own Info.plist right before the move: entries right
  inside the Library's Application Support, Caches, HTTPStorages, WebKit, Cookies, Saved Application
  State, Preferences and Logs folders carrying that identifier, or in Application Support and Logs named
  exactly like the app; Containers and Group Containers are never looked into, since macOS treats them as
  other apps' data. On Windows that is an app whose uninstaller was started from here and whose uninstall
  entry is gone since: a folder right inside AppData's Roaming, Local or LocalLow folders named like its
  name, its publisher or a folder it was installed in or ran from. An entry named like anything the scan
  can see is still there stays out of sight: an installed app (on a Mac, every bundle in the Applications
  folders, by name and identifier; one whose Info.plist can't be read, by name), a running program or
  where it runs from, a launch agent of the user's, a program on the PATH or in App Paths, a packaged
  app, a Start Menu shortcut, a folder in Program Files, a tool known to keep data without any of those,
  a folder the cleaner's catalog knows, Apple's own identifiers, or one of the system's own and the
  user's folders. An app none of these list, such as one run without being installed, can't be seen,
  and when the installed apps can't be read in full nothing is listed on Windows. Named like is all that
  can be told, so none is chosen for you, each goes to the Trash or the Recycle Bin, and nothing is
  remembered across runs.
- The cleaner never follows a link. Each folder is opened one name at a time from the one above it, and
  a link in its place is refused: on Windows that is any reparse point, so junctions, mount points and
  files kept in the cloud are never entered, and one inside a folder being removed goes by itself,
  leaving what it points to.

## Known limits

- A check and the action it guards are separate steps. Between the last check and a signal, a process
  could start another program, or exit and have its PID reused, and the signal would reach a program
  you didn't approve; between the check and a removal, something could start using the item. Right
  before the Recycle Bins are emptied they are checked to hold no more than the scan showed; something
  thrown away in the moment between would go with them. The checks narrow these windows but don't close
  them. On Windows, a stop holds each process open from the check of who it is until it has ended, so
  no other process can take its PID in between. Right before an app's window is asked to close, it is
  checked to still belong to the approved process; in the moment between, the window could close and
  its handle pass to another program's window.
- Between reading an app's uninstall entry again and starting its uninstaller, the entry could change,
  and what starts is what it says in that moment. What an uninstaller started as an administrator loads
  or reads from elsewhere than its own folder isn't checked; Windows' prompt names the publisher of what
  it is asked to run, the last check before anything runs as an administrator.
- On Windows the cleaner can't read what files a program has open, as it can on macOS. It reads each of
  your programs' working folder and where it runs from, and takes a cache to be in use while the tool
  it belongs to runs: Cargo's while `cargo` or `rustc` does, Gradle's while a Gradle daemon does, a
  browser's while the browser does. Programs run by Node, such as npm, can't be told apart by name.
  Windows itself won't delete a file a program opened without letting others delete it, so removing a
  folder stops there and reports the folder as failed, with whatever was removed before it gone. A
  large file a program has open can't be told to be in use either; moving it to the Recycle Bin then
  fails, or the program reads on from the bin. Of the temporary files, only entries that nothing in has
  been made, written or changed in for a week go, each looked at through its own handle, and none a
  process works in or runs from, nor NuGet's lock folder; one Windows won't let go stays, and the
  removal says how many did. A working folder is read only on x64 Windows; elsewhere only where a
  program runs from is known.
- Command lines are shown with likely secrets hidden: the value of every `NAME=value` word; the value
  of a flag or header whose name mentions a credential, such as a token, secret, password, auth, cookie,
  or API or access key; a URL's user info, and query or fragment values under such names or named
  `key`, `sig`, `signature`, `jwt` or `sid`; and the word after `Bearer`. Values of other flags, such as
  `--port=3000`, stay visible apart from any URL secrets in them, since hiding them all would blank
  most dev commands.
- A process that rewrites its own arguments can make parts of its environment read as arguments. Those
  are hidden as `NAME=value` words are, except an entry whose name starts with `-`, which reads as a
  flag.
