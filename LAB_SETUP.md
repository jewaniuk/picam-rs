# Setting up a Windows lab computer

A one-time checklist for preparing a Windows lab computer so agents can develop `picam-rs` there and test it against PICam demo cameras. Work through it top to bottom. Commands are for **Git Bash** unless a step says otherwise.

Each part ends with a check. Don't move on until it passes.

## 1. Prerequisites

Most of these are already on lab computers that build `adam`.

- [ ] **Rust:** `rustc --version` and `cargo --version` both work. If not, install from <https://rustup.rs>.
- [ ] **LLVM** (bindgen needs libclang): `clang --version` works. If not, run `choco install llvm` in an **administrator** PowerShell.
- [ ] **Git for Windows:** `git --version` works.
- [ ] **Claude Code:** `claude --version` works.
- [ ] **PICam SDK:** `C:\Program Files\Princeton Instruments\PICam\Includes\picam.h` exists. In **Settings > Apps > Installed apps**, check the PICam version. The bindings were generated against **5.15.11.2503**. Note the version if it differs, since behaviour may too.
- [ ] **eBUS on `PATH`:** PICam delay-loads Pleora eBUS (the GigE camera libraries) when it initializes, and its installer doesn't put eBUS on `PATH`. LightField copes on its own, but our tests don't.
  - Check whether `C:\Program Files\Common Files\Pleora\eBUS SDK` exists. If it does, add it to your **user** `PATH`: Start menu, "Edit environment variables for your account", **Path**, **Edit…**, **New**. Use this dialog rather than `setx`, which can truncate a long `PATH`.
  - Open a new terminal afterwards. If you use Windows Terminal, close the whole app, since it keeps the environment it started with.
  - **Check:** `which PvBase64.dll` prints the eBUS path.
- [ ] **Sleep turned off:** in **Settings > System > Power**, set "when plugged in, put my device to sleep after" to **Never**. If the computer sleeps, the agent stops and Remote Control drops.

## 2. GitHub CLI and cargo-public-api

- [ ] In an **administrator** PowerShell: `choco install gh`. Then open a **new** Git Bash window and check `gh --version`.
- [ ] Install the public API tool and the nightly toolchain it needs:

  ```sh
  cargo install cargo-public-api --locked
  rustup toolchain install nightly --profile minimal
  ```

## 3. A GitHub token limited to `picam-rs`

The agents push and open PRs with a token from your personal account that can only touch `picam-rs`, rather than the shared lab account's SSH key, which reaches every lab repo.

Create it in a **private browser window** on the lab computer, signed into your personal GitHub account, and sign out afterwards:

- [ ] Go to **Settings > Developer settings > Personal access tokens > Fine-grained tokens > Generate new token**.
- [ ] **Token name:** `picam-rs lab agent`. **Expiration:** 90 days. Renew it when it runs out.
- [ ] **Resource owner:** your personal account. **Repository access:** "Only select repositories", then `picam-rs`.
- [ ] **Permissions > Repository permissions:** **Contents** read and write, **Pull requests** read and write. **Metadata** read-only gets added automatically.
- [ ] **Generate token** and copy it. GitHub only shows it once.
- [ ] Sign `gh` in with it:

  ```sh
  gh auth login
  ```

  Choose **GitHub.com**, then **HTTPS**. Say **no** to authenticating Git with your GitHub credentials (step 4 sets that up for this repo only). Then choose **Paste an authentication token** and paste it.

**Check:** `gh auth status` shows your personal account.

## 4. Clone and configure

- [ ] Clone over HTTPS, not SSH, into a folder of your choice:

  ```sh
  git clone https://github.com/jewaniuk/picam-rs.git
  cd picam-rs
  ```

- [ ] Set this clone (only) to push with the token through `gh`, and to credit commits to you rather than the lab account:

  ```sh
  git config --local credential.https://github.com.helper ''
  git config --local --add credential.https://github.com.helper '!gh auth git-credential'
  git config --local user.name "jewaniuk"
  git config --local user.email "jacob.ewaniuk@queensu.ca"
  ```

  The empty helper line clears any credential helper inherited from the global config (such as Git Credential Manager) for this repo, so the token is the only credential it uses.

**Check:** `git push --dry-run origin main` completes without asking for a password. It's a dry run, so nothing is sent.

## 5. First build and tests

Do this by hand, before any agent touches the repo, so you know the baseline.

- [ ] `cargo build -p picam-rs`
- [ ] **Re-initialization, first:** `cargo test -p picam-rs --test library`. The important one is `reinitialize_after_drop`. It passed on the first lab computer (2026-10-24), but confirm it on each new machine and SDK version.
  - If it **fails**, PICam can't be initialized twice in one process. Stop here: the test helper has to switch to the fallback described in `DESIGN.md`'s Testing section before any agent writes tests.
- [ ] **Demo camera:** `cargo test -p picam-rs --test camera`.
- [ ] `cargo run -p xtask -- progress` reports 82 / 199.
- [ ] `cargo run -p xtask -- gate` **fails at clippy**. That's expected: stage 1 of the roadmap is what makes it pass.
- [ ] `cargo public-api --package picam-rs` prints the public API.

If a test crashes rather than failing normally, the exit code says which DLL problem it is:

- **`0xc0000135`** (`STATUS_DLL_NOT_FOUND`), before any test runs: Windows can't find `Picam.dll` or one of the DLLs it loads at startup. The PICam installer normally puts `C:\Program Files\Common Files\Princeton Instruments\Picam\Runtime` on `PATH`; check that it's there.
- **`0xc06d007e`** (delay-load module not found), right after `running N tests`: PICam loaded, but a DLL it loads later, during `Picam_InitializeLibrary`, is missing. Usually this means eBUS isn't on `PATH` (see part 1).
  - To find which DLL it is, open the "x64 Native Tools Command Prompt for VS" and run `dumpbin /dependents` on `Pida.dll` and `Pidi.dll` in the PICam `Runtime` folder. Look under "delay load dependencies", then search for each listed DLL with `where /r "C:\Program Files" NAME.dll`.
  - Teledyne DALSA Sapera (`SapClassBasic86.dll`, `CorApi.dll`) is also listed there, but PICam runs fine without it on computers with no Camera Link frame grabber.

## 6. Reference material

Agents search the PICam Programmer's Manual instead of reading all 400 pages. It lives in `reference/`, which is gitignored because the manual is Teledyne's copyrighted material.

- [ ] Find the manual. It usually installs with the SDK (look for a documentation folder next to `Includes`). Otherwise download it from Teledyne.
- [ ] Create `reference/` in the repo and copy the PDF into it.
- [ ] Only PICam material goes here. LightField, SPE-format and calibration documents belong to `adam`, not this repo.
- [ ] Start `claude` in the repo (part 7 covers the trust prompt) and give it this prompt:

  > Convert the PICam manual PDF in `reference/` into plain text agents can search. Use a Python PDF library (install it if needed, e.g. pymupdf) and write one `.txt` file per chapter into `reference/manual/`, named by chapter number and title, with page numbers marked in the text as `[page N]`. Keep function signatures, tables and code examples intact as far as possible. Then write `reference/README.md`: what each file contains, with its chapter and page range, and how to search them (Grep for a function name such as `Picam_GetParameterIntegerValue` and then Read around the match). Don't commit anything in `reference/`; it is gitignored.

**Check:**
- `git status` shows nothing new, so `reference/` is properly ignored.
- Grepping `reference/manual/` for `Picam_Acquire` finds its description.

## 7. Claude Code

- [ ] Run `claude` in the repo. When it asks whether you trust the folder, say **yes**. Hooks and the `allow` list stay off until you do.
- [ ] Run `/model` and pick the strongest model available.
- [ ] Run `/hooks` and check that both hooks are listed: `PreToolUse` with `Bash|PowerShell`, and `PostToolUse` with `Edit|Write`.
- [ ] Run `/agents` and check that `reviewer` is listed.
- [ ] **Test the guards.** Still on `main`, ask Claude to run `git push`. The hook should block it with "you are on `main`". Then ask it to run `git push origin main`. The deny rule should refuse it.

## 8. Remote Control

- [ ] On your phone, sign the Claude app into the **same account** as Claude Code on this computer (the lab account).
- [ ] In Claude Code, run `/remote-control` and follow the instructions to connect your phone.
- [ ] From your phone, send a harmless message ("what branch are we on?") and check the answer appears in both places.
- [ ] Leave the terminal open. Closing it ends the session.

## 9. The first chunk

Start the first chunk in plan mode, either `claude --permission-mode plan` or Shift+Tab inside a session, and ask for a plan:

> Read `CLAUDE.md` and `DESIGN.md` in full. Then plan the first chunk of roadmap stage 1: sealing the value traits (`PicamValue`, `ReadablePicamValue`, `OnlineSettablePicamValue`) with the sealed-trait pattern, as described in the roadmap. Follow the workflow in `CLAUDE.md`.

Review this first plan and PR more closely than later ones. They show whether `CLAUDE.md` and the reviewer work as intended, and anything that goes wrong is a fix for those files, made by you.
