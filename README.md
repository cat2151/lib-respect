# lib-respect

A TUI that visualizes the dependency libraries of your repository and allows you to respectfully visit the authors' GitHub profiles. Written in Rust.

## The Problem and This App's Solution
- The Rust ecosystem is amazing! I'm always relying on someone's libraries. I want to give them a star out of respect!
- Wait, did I star this library already...? Oh, I missed it. I want to visualize libraries that haven't been starred yet and respectfully visit the authors' GitHub profiles!
- That's why I created this. While primarily focused on Rust, it also supports some CMake dependencies.

## Installation

Rust is required.

```sh
cargo install --force --git https://github.com/cat2151/lib-respect
```

## Usage

```sh
lib-respect
```

## Check for Updates

```sh
lib-respect check
```

Compares the commit used for building with the latest commit on GitHub's `main` branch. It displays the build-time commit hash in `embedded`, the latest commit hash in `remote`, and `up-to-date` (match) or `update available` (difference) in `result`. Git is required for checking.

## Update

```sh
lib-respect update
```

It executes `cargo install --force --git https://github.com/cat2151/lib-respect` in a separate process, and the application itself terminates. After the update is complete, please manually restart `lib-respect`. Rust, Git, and Python available in your PATH are required for updating. If the update command itself does not work, please re-execute the installation command.