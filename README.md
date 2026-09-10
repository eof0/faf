<p align="center">
<img src="https://res.cloudinary.com/noqoikpl/image/upload/v1789046520/Firefly_RemoveBackground.png" alt="faf" width="520">
</p>

<p align="center">
  <a href="https://crates.io/crates/fafind"><img src="https://img.shields.io/crates/v/fafind" alt="crates.io"></a>
  <a href="https://crates.io/crates/fafind"><img src="https://img.shields.io/crates/msrv/fafind" alt="MSRV"></a>
  <a href="https://aur.archlinux.org/packages/faf-bin"><img src="https://img.shields.io/aur/version/faf-bin" alt="AUR"></a>
  <a href="https://github.com/eof0/faf/actions/workflows/rust.yml"><img src="https://img.shields.io/github/actions/workflow/status/eof0/faf/rust.yml?branch=main" alt="build"></a>
  <a href="https://github.com/eof0/faf/releases"><img src="https://img.shields.io/github/downloads/eof0/faf/total" alt="release downloads"></a>
  <a href="LICENSE"><img src="https://img.shields.io/crates/l/fafind" alt="license"></a>
</p>

# faf

### fast as f*#! filename search.

`faf` is a parallel filesystem search tool written in Rust.
It matches filenames only and stays out of file contents.

---

Pair it with [delfaf](https://github.com/eof0/delfaf) to mass-delete whatever the last `faf` run found.

---

## why this exists

Most search tools scan file contents, allocate per entry, or stall on output locks.
faf walks the tree on every core, matches raw filename bytes, and writes results in large batches.

---

## install

### cargo

~~~bash
cargo install fafind
~~~

The crate is named `fafind` because `faf` was already taken on crates.io.
It installs the `faf` binary.

### packages

~~~bash
# Arch (AUR)
yay -S faf-bin

# Homebrew
brew install eof0/tap/faf
~~~

AUR packaging notes: [`packaging/aur/README.md`](packaging/aur/README.md).

### from source

Requires Rust 1.88 or newer (edition 2024).

~~~bash
git clone https://github.com/eof0/faf
cd faf
cargo build --release
sudo cp target/release/faf /usr/local/bin/
~~~

Unix only - Linux and macOS. The build fails on Windows by design.

---

## usage

~~~bash
faf <target> [root]
~~~

`root` defaults to `/`.

---

## matching modes

### default (stem match)

Matches the filename without its extension.
An extension on the query is ignored, so `faf main.rs` is the same as `faf main`.

~~~bash
faf main .
~~~

Matches `main.rs` and `main.go`.
Does not match `domain.rs`.

### substring (`-s`)

~~~bash
faf -s foo .
~~~

Matches `foobar.txt`, `myfoo.rs`, `prefoo`, and `notes.foo`.
The whole filename is searched, extension included.

### exact (`-p`)

~~~bash
faf -p Makefile .
~~~

Matches `Makefile` only.

`-s` and `-p` cannot be combined.

---

## terminal colors

When stdout is a terminal and `-0` is not set, matches are highlighted:

| Color | Applies to |
|-------|------------|
| Dim | Path before the filename |
| Green | The matched part of the name |
| Bold green | Stem in `-p` mode |
| Yellow | Extension in stem and `-p` modes |
| Orange | Non-matching parts of the name in `-s` mode |

`--color auto` is the default.
Use `--color always` or `--color never` to override.

---

## what gets walked

- hidden files and directories are included
- symlinks are never followed
- `.gitignore` is ignored unless `--gitignore` is passed

---

## flags

### case insensitive (`-i`)

~~~bash
faf -i readme .
~~~

ASCII names use a byte-wise fold.
Non-ASCII names and queries fall back to full Unicode case folding.

### limit depth

~~~bash
faf --max-depth 3 main .
~~~

### exclude directories

~~~bash
faf --exclude target,node_modules main .
~~~

Matches directory names anywhere below the root.
The root itself is never excluded.

### respect .gitignore

~~~bash
faf --gitignore main .
~~~

### filter by type (`-f` / `-d` / `--type`)

~~~bash
faf -f main .         # files only
faf -d src .          # directories only
faf --type a main .   # any (default)
~~~

`-f` and `-d` stack with the other short flags, so `-sd`, `-pf`, and `-id` all work.
They cannot be combined with each other or contradict an explicit `--type`.

### null-separated output (`-0`)

~~~bash
faf -0 main . | xargs -0 rm
~~~

Disables color.

### verbose (`-v`)

Prints `[SCAN]`, `[SKIP]`, and `[ERROR]` lines to stderr.
Matches still go to stdout, but as `[MATCH] <path>` lines with color and `-0` ignored.
Use it to trace a walk, not to feed another program.

### quiet (`-q`)

Suppresses the summary line on stderr.

---

## performance

- Every core walks the tree through a work-stealing scheduler.
- Filenames are matched as raw bytes, with no UTF-8 decoding or per-entry allocation in the matcher.
- Substring search uses a prebuilt SIMD `memmem` finder.
- Non-ASCII names take a cold Unicode path only when `-i` needs it.
- Each worker batches output into a private buffer and writes it in 64 KiB chunks, whether stdout is a terminal or a pipe.
- When the reader closes the pipe, as in `faf -s foo / | head`, the walk stops instead of scanning the rest of the disk.

---

## output

- newline-separated by default, NUL-separated with `-0`
- raw OS bytes, no re-encoding
- the matched paths are also written to a cache file for `delfaf`, always NUL-separated

The cache path is `$FAF_LAST`, else `$XDG_CACHE_HOME/faf/last`, else `~/.cache/faf/last`.
A failed cache write never changes the exit code.

---

## exit codes

~~~text
0 = matches found
1 = no matches
2 = invalid usage
~~~

---

## what this is NOT

- not a content search tool (use `grep` or `rg`)
- not a fuzzy matcher

---

## changelog

See [CHANGELOG.md](CHANGELOG.md).

## license

MIT
