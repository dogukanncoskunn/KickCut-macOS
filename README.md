# KickCut for macOS

Download a Kick broadcast — all of it, or the three hours in the middle you
actually want — as an MP4 that opens cleanly in an editor.

This is the macOS build. It is the same app as
[KickCut](https://github.com/dogukanncoskunn/KickCut), which is where the
Windows build lives; the differences are the ones macOS forces — which FFmpeg
is fetched, how a file is revealed in Finder, and how it is packaged.

## Install

Requires macOS 12 Monterey or later on Apple Silicon. Intel Macs are not
supported.

Grab the latest `KickCut_x.y.z_aarch64.dmg` from
[Releases](https://github.com/dogukanncoskunn/KickCut-mac/releases), open it
and drag KickCut into Applications.

**macOS will refuse it the first time.** KickCut is not signed with an Apple
Developer certificate, so a double-click gets "KickCut cannot be opened".
Right-click the app in Applications, choose **Open**, then **Open** again in
the dialog — that records your decision once and every later launch is normal.
The same thing in one command:

```bash
xattr -dr com.apple.quarantine /Applications/KickCut.app
```

A certificate is an annual cost and paying it would remove a warning rather
than change what the app does, so the hash is published instead. If you want
to be sure the file is the one published here and was not altered on its way
to you, every release lists the disk image’s SHA-256. Compare it:

```bash
shasum -a 256 KickCut_0.3.0_aarch64.dmg
```

That proves the file matches what was built from this repository. It does not
make an unknown program safe — it answers "is this the real one", which is the
question worth asking when an installer reaches you through chat rather than
from the release page.

On first launch, open **Settings** and install FFmpeg. It is a one-time 57 MB
download — two archives, ffmpeg and ffprobe — each pinned to a specific build
and checked against its published SHA-256; KickCut keeps them in its own
folder and never touches your PATH. If FFmpeg is already on your machine,
through Homebrew or anything else, KickCut finds it and downloads nothing.

## Using it

1. **Library** — type a channel name to list its recent broadcasts, or paste a
   VOD link. Kick's channel endpoint is not paginated, so it only reaches the
   most recent ones; anything older has to come in as a link.
2. **Download** — pick a quality, then set the range. Drag the ends of the
   timeline or type `02:00:00` and `05:30:00` into the boxes. Amber marks on
   the timeline are breaks in the broadcast (see below).
3. Choose a folder and a file name, then **add it to the queue**. Jobs run one
   at a time; the rail beside the form shows progress and has the pause button.
4. Pause whenever you like — including by closing the app. Reopen it and the
   job is waiting, and resuming carries on from the segment it reached.

There is a download speed limit in the rail header and in Settings. It applies
immediately, to a download already running.

## Why the output is different

The obvious way to do this — join the stream's segments and copy them into an
MP4 — produces a file that plays fine and then misbehaves in an editor: audio
artefacts, and a timeline that stutters or stalls part-way through. Three
things cause it, and KickCut fixes each:

- Audio inside a transport stream is framed differently from audio inside MP4.
  Copying it across without converting leaves the wrong header on every packet.
  Players tolerate that; editors do not.
- A broadcast's timestamps start wherever the encoder happened to be and jump
  at every break in the stream. They are rebuilt into one continuous run from
  zero.
- MPEG-TS counts time on a 90 kHz clock. The MP4 is written on the same one, so
  no rounding drift accumulates across an eight-hour recording.

**Breaks in the broadcast.** When a stream is interrupted the encoding can
change either side of the gap, and no amount of flags makes a stream copy span
that cleanly. KickCut marks these on the timeline and warns when your range
crosses one, and offers a re-encoding mode that dissolves the break and cuts on
the exact frame instead of the nearest keyframe. It takes hours instead of
minutes, so it is offered rather than imposed.

**Cut accuracy.** In the fast mode the start lands on the nearest keyframe at
or after the time you asked for — within about two seconds. Use the
editing-safe mode when you need the exact frame.

## What it stores and what it sends

Nothing leaves your machine except the requests needed to do the job. There is
no analytics, no telemetry and no crash reporting — the app talks to exactly
three hosts:

| Host | Why |
|---|---|
| `kick.com` | broadcast list and VOD metadata |
| `stream.kick.com` | the playlists and the video segments |
| `ffmpeg.martin-riedl.de` | the one-time FFmpeg download |

On disk it keeps its FFmpeg copy, a small JSON record per queued job, and the
segments of downloads still in progress — all under
`~/Library/Application Support/com.unsatisfied0.kickcut`, plus a WebView profile
in `~/Library/WebKit/com.unsatisfied0.kickcut` holding your settings and Kick
cookies. Dragging the app to the Trash leaves both behind, as it does for every
macOS app; delete those two folders to remove the rest. Videos you have already
saved are never touched, because they live in the folder you chose.

## Building it

```bash
npm install
npm run tauri:dev     # run it
npm run tauri:build   # produce the .dmg
```

`tauri:build` goes through `scripts/build-release.mjs` rather than calling
Tauri directly. Rust bakes absolute source paths into a release build, so
without remapping them the shipped binary tells everyone who downloads it what
the build machine's user account is called.

Checks, all of which CI runs on every push:

```bash
npm run typecheck
npm run build
cd src-tauri && cargo test --lib && cargo clippy --lib -- -D warnings
```

Three tests are marked `#[ignore]` because they need the network, a real
FFmpeg, or both. They are the ones that prove the app actually works rather
than that its arguments look right, so run them after touching anything they
cover:

```bash
# Downloads the pinned FFmpeg, verifies its checksum and unpacks it.
cargo test -- --ignored installs_the_pinned_build

# An FFmpeg already on the machine is used as-is, and a managed copy wins.
KICKCUT_TEST_FFMPEG_DIR=<folder with ffmpeg+ffprobe>   cargo test -- --ignored an_ffmpeg_already_on_path

# Real segments off Kick's CDN, joined by the real FFmpeg and read back.
KICKCUT_TEST_FFMPEG_DIR=<folder> KICKCUT_TEST_PLAYLIST=<media playlist url>   cargo test -- --ignored assembles_real_segments
```

## A note on what you download

This is a tool for keeping your own broadcasts, or content you have permission
to keep. It downloads what Kick already serves to any viewer and does not
circumvent any protection. What you do with the file is your responsibility.

---

made by unsatisfied0
