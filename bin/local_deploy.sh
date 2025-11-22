#!/bin/sh
set -eux

cargo build --release
sudo install -m 755 -o root target/release/sparktop /usr/local/bin/sparktop
sudo chmod u+s /usr/local/bin/sparktop
