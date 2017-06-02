#!/bin/sh
set -eu

T=$(mktemp -p .)
D=$(mktemp -d)
trap 'rm '"$T" EXIT

truncate -s 8M "$T"

# create a single partition that's as big as it can be:
printf 'n\n\n\n\n\nw\n' | fdisk "$T"

L=$(sudo losetup -P -f --show "$T")
sudo mkfs.ext4 -O '^64bit' "${L}p1"
sudo mount "${L}p1" "$D"


sudo touch "$D"/hello


sudo umount "$D"
rm -r "$D"
sudo losetup -d "$L"
mv "$T" simple.img
trap '' EXIT
