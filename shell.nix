with import <nixpkgs> {};
stdenv.mkDerivation rec {
  name = "env";
  env = buildEnv { name = name; paths = buildInputs; };
  buildInputs = [
    fuse
    #fuse3
    #fuse-common
    rustup
    pkg-config
  ];
}
