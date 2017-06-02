#!/bin/sh
set -eu

T=$(mktemp -p .)
D=$(mktemp -d .mounting.XXXXXXXXX)
trap 'rm "${T}"; rm -r "$D"' EXIT

truncate -s 8M "$T"

# create a single partition that's as big as it can be:
printf 'n\n\n\n\n\nw\n' | fdisk "$T"

L=$(sudo losetup -P -f --show "$T")
sudo mkfs.ext4 -O '^64bit' "${L}p1"
sudo mount "${L}p1" "$D"

H=$(pwd)
(cd "$D" && sudo "$H/$1")


sudo umount "$D"
rm -r "$D"
sudo losetup -d "$L"
cp --sparse=always "$T" "$2"
rm "$T"
trap '' EXIT
