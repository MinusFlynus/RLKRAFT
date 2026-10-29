"""Build-time integration of pinned official native feature bundles.

Uses the same versioned hook compatibility and mixin compiler as the launcher.
Downloads are confined to the CI runner's temporary directory; game assets and
Fabric jars are never needed or included in the public build.
"""
import hashlib
import io
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import urllib.request
import zipfile


BUNDLES = {
    "minimap": "6f5dbca0699562b088a04b3f5abdc1b7faf8da51257a96da0a4ed639c6644ed9",
    "recipes": "51fddf300709404170061355afdf0b407f28eb928bcc0a9133c241cd63f4a0ed",
}
MIXINS = "0d6b103c70633d8fd9443d8d0acd3aadaacedb06"


def main():
    root = Path(__file__).resolve().parents[1]
    revision = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=root, text=True
    ).strip()
    hooks_text = (root / "crates/horizon-client/src/game/native_hooks.rs").read_text()
    hooks = int(re.search(r"NATIVE_HOOKS_API: u32 = (\d+);", hooks_text)[1])
    provenance = {"revision": revision, "mixins": MIXINS, "bundles": []}
    with tempfile.TemporaryDirectory(prefix="horizon-native-") as scratch:
        scratch = Path(scratch)
        mods = scratch / "mods"
        mods.mkdir()
        for name, digest in BUNDLES.items():
            url = (
                "https://github.com/horizon-rust/horizon-mods/releases/download/"
                f"{name}-2.0.0/{name}-2.0.0.hnmod"
            )
            with urllib.request.urlopen(url, timeout=120) as response:
                data = response.read()
            assert hashlib.sha256(data).hexdigest() == digest, name
            bundle = scratch / name
            with zipfile.ZipFile(io.BytesIO(data)) as archive:
                for entry in archive.infolist():
                    destination = (bundle / entry.filename).resolve()
                    assert destination.is_relative_to(bundle.resolve()), entry.filename
                archive.extractall(bundle)
            manifest = json.loads((bundle / "horizon-native.json").read_text())
            assert manifest["hooks"] <= hooks, (name, hooks)
            assert manifest["mixins"] == MIXINS, name
            for source in (bundle / "mixins").iterdir():
                target = mods / source.name
                shutil.copytree(source, target)
                path = target / "horizon-mixin.toml"
                text, count = re.subn(
                    r'horizon_revision = "[0-9a-f]{40}"',
                    f'horizon_revision = "{revision}"', path.read_text(),
                )
                assert count == 1, name
                path.write_text(text)
            provenance["bundles"].append({"id": name, "sha256": digest, "url": url})
        (root / ".horizon-revision").write_text(revision)
        stage = scratch / "staged"
        subprocess.run([
            "horizon-mixins", "apply", "--workspace", str(root),
            "--mods", str(mods), "--out", str(stage),
        ], check=True, cwd=root)
        changed = []
        for source in (stage / "crates").rglob("*.rs"):
            relative = source.relative_to(stage)
            destination = root / relative
            if not destination.exists() or source.read_bytes() != destination.read_bytes():
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, destination)
                changed.append(relative.as_posix())
        assert changed, "mixin application produced no Rust changes"
        provenance["changed_rust_files"] = changed
    (root / "native-profile-provenance.json").write_text(json.dumps(provenance, indent=2))
    print(f"Integrated {len(BUNDLES)} native bundles into {len(changed)} Rust files")


if __name__ == "__main__":
    main()
