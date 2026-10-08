"""Build the three embedded Bevy examples into a static GitHub Pages site."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parent.parent
GAMES = {
    "shooter": ("Shooter / 飞行射击", "WASD / 方向键移动 · 自动开火", "点击游戏画面后使用键盘移动。"),
    "gomoku": ("Gomoku / 五子棋", "点击棋盘落子 · 与 AI 对弈", "Save / Load 在当前浏览器保存、恢复棋局与脚本。"),
    "xiangqi": ("Xiangqi / 中国象棋", "点击棋子，再点击目标位置", "Save / Load 在当前浏览器保存、恢复棋局与脚本。"),
}

def run(*args):
    subprocess.run(args, cwd=ROOT, check=True)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-build", action="store_true", help="Package already-built wasm binaries")
    parser.add_argument("--output", default="dist/web", help="Static site destination inside this repository")
    args = parser.parse_args()
    output = (ROOT / args.output).resolve()
    if not output.is_relative_to(ROOT) or output == ROOT:
        parser.error("output must be a subdirectory of the repository")
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    version = next(p["version"] for p in lock["package"] if p["name"] == "wasm-bindgen")
    actual = subprocess.check_output(["wasm-bindgen", "--version"], text=True).strip()
    if actual != f"wasm-bindgen {version}":
        raise SystemExit(f"Install matching bindings CLI: cargo install wasm-bindgen-cli --version {version} --locked")
    if not args.skip_build:
        run("cargo", "build", "--locked", "--profile", "web-release", "--target", "wasm32-unknown-unknown",
            *[arg for game in GAMES for arg in ("--example", game)])
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    if not target.is_absolute():
        target = ROOT / target
    output.mkdir(parents=True, exist_ok=True)
    for filename in ("index.html", "style.css", "loader.js", "CNAME"):
        shutil.copy2(ROOT / "web" / filename, output / filename)
    (output / "images").mkdir(exist_ok=True)
    template = (ROOT / "web/game.html").read_text(encoding="utf-8")
    for game, (title, controls, note) in GAMES.items():
        folder = output / game
        folder.mkdir(exist_ok=True)
        wasm = target / "wasm32-unknown-unknown/web-release/examples" / f"{game}.wasm"
        run("wasm-bindgen", "--target", "web", "--no-typescript", "--out-name", "game", "--out-dir", str(folder), str(wasm))
        page = template.replace("__GAME__", game).replace("__TITLE__", title).replace("__CONTROLS__", controls).replace("__NOTE__", note)
        (folder / "index.html").write_text(page, encoding="utf-8")
        shutil.copy2(ROOT / "docs/screenshots" / f"{game}.png", output / "images" / f"{game}.png")
        print(f"{game}: {(folder / 'game_bg.wasm').stat().st_size / 1048576:.1f} MiB")
    (output / ".nojekyll").touch()
    print(f"Site ready: {output}")

if __name__ == "__main__":
    main()
