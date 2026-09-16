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
[Releases](https://github.com/dogukanncoskunn/KickCut-macOS/releases), open it
and drag KickCut into Applications.

**macOS will refuse it the first time.** KickCut is not signed with an Apple
Developer certificate, so the first double-click is stopped with a warning that
Apple cannot check it. You allow it once, and every later launch is normal:

- **macOS 15 Sequoia and later:** close the warning, open **System Settings →
  Privacy & Security**, scroll down to the line about KickCut and choose
  **Open Anyway**, then confirm with your password.
- **macOS 12 – 14:** right-click the app in Applications, choose **Open**, then
  **Open** again in the dialog.

Or, on any version, in one command:

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
no account, no analytics, no telemetry and no crash reporting. These are the
only hosts the app contacts:

| Host | Why |
|---|---|
| `kick.com` | broadcast list and VOD metadata |
| `images.kick.com` | broadcast thumbnails |
| `stream.kick.com` | the playlists and the video segments |
| `ffmpeg.martin-riedl.de` | the one-time FFmpeg download |
| `github.com` | one small check per launch for a newer version, and the update itself if you accept it |

That list is not a promise written from memory. Every change to the test
workflow re-runs the real app on a Mac, records every connection the app and
its web view make, and fails if one goes anywhere else.

KickCut shows its screens through Apple's own web engine, the one Safari uses.
When that engine starts, macOS itself refreshes Apple's fraud-protection and
privacy lists (`safebrowsing.apple`, `wps.apple.com`). That is macOS's traffic,
the same for every app built this way, and it carries nothing about what you do
in KickCut.

On disk, measured on the same run:

| Where | What |
|---|---|
| `~/Library/Application Support/com.unsatisfied0.kickcut` | FFmpeg; one small record per download (title, channel, quality, folder, file name), kept until you remove it from the list; the segments of a download still in progress, deleted when it finishes |
| `~/Library/WebKit/com.unsatisfied0.kickcut` | your settings — language, theme, output folder |
| `~/Library/Caches/com.unsatisfied0.kickcut` | the web engine's cache of Kick's replies and thumbnails, a few megabytes |
| `~/Library/Saved Application State/com.unsatisfied0.kickcut.savedState` | window position, kept by macOS |

No cookies are stored. Dragging the app to the Trash leaves these behind, as it
does for every macOS app; this removes them:

```bash
rm -rf ~/Library/Application\ Support/com.unsatisfied0.kickcut \
       ~/Library/WebKit/com.unsatisfied0.kickcut \
       ~/Library/Caches/com.unsatisfied0.kickcut \
       ~/Library/Saved\ Application\ State/com.unsatisfied0.kickcut.savedState
```

Videos you have already saved are never touched, because they live in the
folder you chose.

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
