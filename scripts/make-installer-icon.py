from pathlib import Path

from PIL import Image

root = Path(__file__).resolve().parents[1]
source = root / "assets" / "ico.png"
target = root / "packaging" / "installer" / "flux-launcher.ico"

image = Image.open(source).convert("RGBA")
image.save(target, format="ICO", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
print(f"wrote {target} from {source} ({image.width}x{image.height})")
