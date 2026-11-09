"""Read LENOVO_LIGHTING_DATA instance zero using the stock wmi-bmof driver.

Requires root for sysfs driver binding. Temporarily binds only the unclaimed
lighting DATA block, reads the driver's cached query result, and undoes binding.
Does not invoke a WMI setter or write lighting/EC/physical-memory settings.
"""
from pathlib import Path
import json
import os
import tempfile

if os.geteuid() != 0:
    raise SystemExit("Run with sudo: sysfs driver binding requires root.")

devices = list(Path('/sys/bus/wmi/devices').glob(
    '4DD5BD84-15A9-47E2-AD65-CC61A5C62FD0-*'))
if len(devices) != 1:
    raise SystemExit('Expected exactly one Lenovo lighting-data block.')
device = devices[0]
if (device / 'object_id').read_text().strip() != 'A8':
    raise SystemExit('Unexpected ACPI object ID; refusing to bind.')
if (device / 'driver').exists():
    raise SystemExit('Lighting-data block already has a driver; leaving it alone.')
override = device / 'driver_override'
previous = override.read_text().strip()
if previous not in ('', '(null)'):
    raise SystemExit('An existing driver override is set; leaving it alone.')
driver = Path('/sys/bus/wmi/drivers/wmi-bmof')
if not driver.exists():
    raise SystemExit('Stock wmi-bmof driver is unavailable.')

bound = False
try:
    override.write_text('wmi-bmof\n')
    (driver / 'bind').write_text(device.name + '\n')
    bound = True
    result = (device / 'bmof').read_bytes()
    if len(result) != 7 or result[0] != 0:
        raise RuntimeError('Unexpected lighting descriptor: ' + result.hex(' '))
    kind = (result[2] >> 1) & 7
    report = {
        'device': device.name,
        'descriptor': result.hex(' '),
        'lighting_id': result[0],
        'lighting_type': result[2],
        'rgb': bool(result[2] & 1),
        'keyboard_type': {1: '4zone', 2: 'Perkey', 3: '24zone', 4: 'OneZone'}.get(kind, 'unknown'),
    }
    fd, filename = tempfile.mkstemp(prefix='legion-lighting-type-', suffix='.json')
    with os.fdopen(fd, 'w') as stream:
        json.dump(report, stream, indent=2)
        stream.write('\n')
    os.chmod(filename, 0o644)
    print(json.dumps(report, indent=2))
    print(filename)
finally:
    try:
        if bound or ((device / 'driver').exists() and
                     (device / 'driver').resolve() == driver.resolve()):
            (driver / 'unbind').write_text(device.name + '\n')
    finally:
        override.write_text('\n')
