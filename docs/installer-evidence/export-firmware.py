"""Export read-only ACPI/WMI evidence for the keyboard investigation.

Run with sudo only because Linux restricts these firmware files to root.
Does not invoke ACPI methods, load drivers, or write hardware settings.
Writes copies into a fresh /tmp directory and prints its path.
"""

from pathlib import Path
import json
import tempfile

output = Path(tempfile.mkdtemp(prefix="legion-firmware-"))
output.chmod(0o755)
sources = [Path("/sys/firmware/acpi/tables/DSDT")]
sources += sorted(Path("/sys/firmware/acpi/tables").glob("SSDT*"))
sources += sorted(Path("/sys/bus/wmi/devices").glob("*/bmof"))
manifest = []
for source in sources:
    target = (source.name if source.name != "bmof"
              else source.parent.name + ".bmof")
    try:
        data = source.read_bytes()
        destination = output / target
        destination.write_bytes(data)
        destination.chmod(0o644)
        manifest.append({"source": str(source), "file": target, "bytes": len(data)})
    except OSError as error:
        manifest.append({"source": str(source), "error": str(error)})
(output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
(output / "manifest.json").chmod(0o644)
print(output)
