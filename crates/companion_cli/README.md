# RUST Oxide CLI

Companion CLI for the Rust Oxide server template. Use it to initialize a new
project and scaffold or remove CRUD APIs.

## Install

With Rust installed:

```sh
cargo install rust-oxide-cli
```

This installs the `oxide` binary.

From this repo:

```sh
cargo run -p rust-oxide-cli -- init my_app
```

## Usage

```sh
# initialize a project
oxide init my_app

# add a CRUD API
oxide api add todo_item --fields "title:string,done:bool"

# remove a CRUD API
oxide api remove todo_item
```

Run `oxide --help` for full flags.
