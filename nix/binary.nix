{ pkgs
, lib
, system ? pkgs.stdenv.hostPlatform.system
, version ? "0.1.0"
, owner ? "Initsnow"
, repo ? "ReadingSteiner"
, hashes ? { }
}:

let
  defaultHashes = {
    x86_64-linux = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    aarch64-linux = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    x86_64-darwin = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    aarch64-darwin = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
  };

  targetMap = {
    x86_64-linux = "x86_64-unknown-linux-musl";
    aarch64-linux = "aarch64-unknown-linux-musl";
    x86_64-darwin = "x86_64-apple-darwin";
    aarch64-darwin = "aarch64-apple-darwin";
  };

  allHashes = defaultHashes // hashes;

  target = targetMap.${system} or (throw "Unsupported system for reading-steiner-bin: ${system}");
  hash = allHashes.${system} or (throw "No binary hash available for system: ${system}");

  assetName = "reading-steiner-v${version}-${target}.tar.gz";
  url = "https://github.com/${owner}/${repo}/releases/download/v${version}/${assetName}";
in
pkgs.stdenv.mkDerivation (finalAttrs: {
  pname = "reading-steiner-bin";
  inherit version;

  src = pkgs.fetchurl {
    inherit url hash;
  };

  sourceRoot = ".";

  nativeBuildInputs = [ ]
    ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.autoPatchelfHook ];

  buildInputs = [ ]
    ++ pkgs.lib.optionals pkgs.stdenv.isLinux [ pkgs.stdenv.cc.cc.lib ];

  installPhase = ''
    runHook preInstall

    # 安装主执行文件
    find . -type f -name reading-steiner -exec install -Dm755 {} $out/bin/reading-steiner \;

    # 安装随发布包分发的 Web 控制台前端静态资源（若存在）
    if [ -d web/dist ]; then
      mkdir -p $out/share/reading-steiner/web
      cp -r web/dist/* $out/share/reading-steiner/web/
    elif [ -d dist ]; then
      mkdir -p $out/share/reading-steiner/web
      cp -r dist/* $out/share/reading-steiner/web/
    fi

    runHook postInstall
  '';

  passthru = {
    hasWeb = true;
    inherit url;
  };

  meta = with lib; {
    description = "ReadingSteiner - web/data change detection with Telegram push (precompiled binary from GitHub Releases)";
    homepage = "https://github.com/${owner}/${repo}";
    license = licenses.mit;
    mainProgram = "reading-steiner";
    platforms = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
  };
})
