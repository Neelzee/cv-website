{
  description = "CV Webpage";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [
            (import rust-overlay)
          ];
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        nativeBuildInputs = with pkgs; [
          rustToolchain
          pkg-config
          tailwindcss_4
          nodejs
          esbuild
          openapi-generator-cli
        ];

        buildInputs = with pkgs; [
          openssl
          postgresql.lib
        ];
      in
      {
        devShells.default = pkgs.mkShell {
          inherit buildInputs;
          nativeBuildInputs = nativeBuildInputs ++ (with pkgs; [
            sqlx-cli
            just
            curl
            playwright-driver.browsers
            diesel-cli
          ]);

          PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD = "1";
          PLAYWRIGHT_BROWSERS_PATH = "${pkgs.playwright-driver.browsers}";
        };
      }
    );
}
