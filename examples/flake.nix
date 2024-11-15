{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs";
    nixpkgs.path = ./../nixpkgs;
  };

  outputs = {nixpkgs, ...}: let
    system = "x86_64-linux";
    pkgs = import nixpkgs {
      inherit system;
    };
  in {
    devShells.${system}.default = pkgs.mkShell {
      buildInputs = with pkgs; [
        hello
      ];
    };
  };
}
