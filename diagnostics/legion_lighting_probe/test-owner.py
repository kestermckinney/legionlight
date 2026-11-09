"""Sign/load the exact-model probe, optionally test ownership plus live red.

Default: query only. --test: if ownership is 0, temporarily request 1, send the
recovered OneZone live-red packets, ask for the visible result, then restore.
Uses the existing enrolled akmods key; never creates or enrolls a key.
"""
import argparse
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--test', action='store_true')
parser.add_argument('--lamp', action='store_true', help='also test the standard LampArray red path with ownership enabled')
args = parser.parse_args()
if os.geteuid() != 0:
    raise SystemExit('Run with sudo; signing and loading require administrator access.')

root = Path(__file__).resolve().parent
module = root / 'legion_lighting_probe.ko'
state = Path('/sys/module/legion_lighting_probe')
if state.exists():
    raise SystemExit('Probe is already loaded; leaving that instance alone.')
certificate = Path('/etc/pki/akmods/certs/public_key.der')
key = Path('/etc/pki/akmods/private/private_key.priv')
if not module.is_file() or not certificate.is_file() or not key.is_file():
    raise SystemExit('Built module or existing akmods signing files are missing.')
# Compare the public certificate with exported enrolled certificates directly.
# This mokutil build can print "already enrolled" while returning status 1.
with tempfile.TemporaryDirectory(prefix='legion-enrolled-certs-') as enrolled:
    exported = subprocess.run(['mokutil', '--export'], cwd=enrolled,
                              capture_output=True, text=True)
    if exported.returncode != 0:
        raise SystemExit('Could not read enrolled certificates: ' +
                         (exported.stderr or exported.stdout).strip())
    certificate_bytes = certificate.read_bytes()
    if not any(path.read_bytes() == certificate_bytes
               for path in Path(enrolled).glob('*.der')):
        raise SystemExit('The existing akmods certificate is not in the enrolled key list.')
signer = Path('/lib/modules') / os.uname().release / 'build/scripts/sign-file'
# Sign a temporary copy so root does not modify the user's build artifact.
with tempfile.TemporaryDirectory(prefix='legion-signed-probe-') as temporary:
    signed = Path(temporary) / module.name
    signed.write_bytes(module.read_bytes())
    subprocess.run([str(signer), 'sha256', str(key), str(certificate), str(signed)], check=True)
    loaded = False
    fd = None
    lamp_fd = None
    original = None
    profile = None
    brightness = None
    live_started = False
    result = {}

    def send(packet, delay=.15):
        size = len(packet)
        returned = fcntl.ioctl(fd, (3 << 30) | (size << 16) | (ord('H') << 8) | 6,
                              bytearray(packet), True)
        if returned != size:
            raise RuntimeError('Incomplete USB feature report')
        time.sleep(delay)

    def query(command):
        send(packets.report(command, b'' if command == 0xCC else b'\0'), .05)
        buffer = bytearray(960)
        buffer[0] = 7
        size = fcntl.ioctl(fd, (3 << 30) | (960 << 16) | (ord('H') << 8) | 7, buffer, True)
        if size < 5 or buffer[:2] != bytes((7, command)):
            raise RuntimeError('Unexpected query response')
        return bytes(buffer[:size])

    def lamp_send(packet):
        size = len(packet)
        returned = fcntl.ioctl(lamp_fd, (3 << 30) | (size << 16) | (ord('H') << 8) | 6,
                               bytearray(packet), True)
        if returned != size:
            raise RuntimeError('Incomplete LampArray feature report')
        time.sleep(.05)

    try:
        subprocess.run(['insmod', str(signed)], check=True)
        loaded = True
        ownership = state / 'parameters/ownership'
        initial = int(ownership.read_text().strip())
        result['original_ownership'] = initial
        print('Firmware lighting ownership:', initial, flush=True)
        if args.test and initial == 0:
            identity = Path('/sys/class/hidraw/hidraw0/device/uevent').read_text()
            if '0000048D:0000C615' not in identity:
                raise RuntimeError('Expected keyboard is not hidraw0; refusing USB writes.')
            packet_source = root.parent.parent / 'docs/installer-evidence/packets.py'
            spec = importlib.util.spec_from_file_location('packets', packet_source)
            packets = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(packets)
            fd = os.open('/dev/hidraw0', os.O_RDWR | os.O_CLOEXEC)
            # Query BEFORE changing ownership; a selected profile of 0 can mean off.
            selected = query(0xCA)[4]
            result['initial_selected_profile'] = selected
            print('Selected profile before ownership change:', selected, flush=True)
            for attempt in range(3):
                if selected in (1, 2, 3):
                    break
                print('No ownership or live-colour change yet.', flush=True)
                input('Select the lit Custom palette setting in the app (Apply profile), then Enter: ')
                selected = query(0xCA)[4]
                print('Selected profile:', selected, flush=True)
            result['selected_profile_before_test'] = selected
            if selected not in (1, 2, 3):
                raise RuntimeError('No selected profile 1–3; stopping before changing ownership.')
            snapshot = query(0xCC)
            result['stored_profile_before_test'] = snapshot[4]
            result['stored_prefix'] = snapshot[:40].hex(' ')
            if len(snapshot) != 960 or snapshot[4] != selected:
                raise RuntimeError('Selected and stored profile do not match; stopping before changing ownership.')
            profile = selected
            original = snapshot
            brightness = query(0xCD)[4]
            if brightness > 9:
                raise RuntimeError('Unexpected brightness; stopping before changing ownership.')
            count = original[21]
            result['original_colour_count'] = count
            if count <= 4:
                keys = 22 + count * 3
                result['stored_key_bytes'] = original[keys:keys+3].hex(' ')
            # Live A1 and LampArray do not parse/rewrite the stored CB key list.
            ownership.write_text('1\n')
            live_started = True
            result['test_ownership'] = int(ownership.read_text().strip())
            result['selected_profile_after_ownership'] = query(0xCA)[4]
            print('Selected profile after ownership change:',
                  result['selected_profile_after_ownership'], flush=True)
            result['usb_channel'] = 'AA1 (Lenovo profile-start path)'
            send(packets.autonomous(True))
            send(packets.report(0xC8, bytes((profile,))))
            send(packets.chip_control(4, profile))
            send(packets.live_colour(0xA1, [(1, 255, 0, 0)]), .005)
            send(packets.live_colour(0xA8, []), .005)
            result['visible_result'] = input('Vendor live test: is the keyboard solid red? Type what you see, then Enter: ')
            if args.lamp:
                send(packets.chip_control(0xDC, profile))
                identity = Path('/sys/class/hidraw/hidraw1/device/uevent').read_text()
                if '0000048D:0000C615' not in identity:
                    raise RuntimeError('Expected LampArray keyboard is not hidraw1.')
                lamp_fd = os.open('/dev/hidraw1', os.O_RDWR | os.O_CLOEXEC)
                send(packets.autonomous(False))
                lamp_send(bytes.fromhex('06 00'))
                lamp_send(bytes.fromhex('05 01 00 00 00 00 ff 00 00 01'))
                result['lamp_visible_result'] = input('LampArray live test: is the keyboard solid red? Type what you see, then Enter to restore: ')
        elif args.test:
            result['test_skipped'] = 'Ownership is already 1; no new test performed.'
    finally:
        try:
            try:
                if lamp_fd is not None:
                    try:
                        lamp_send(bytes.fromhex('06 01'))
                    finally:
                        os.close(lamp_fd)
            finally:
                if loaded:
                    # Restore original ownership before restoring profile selection.
                    subprocess.run(['rmmod', 'legion_lighting_probe'], check=True)
                    loaded = False
                    result['probe_unloaded'] = True
        finally:
            try:
                if fd is not None and live_started:
                    send(packets.autonomous(True))
                    send(packets.report(0xC8, bytes((profile,))))
                    send(packets.report(0xCE, bytes((brightness,))))
                    send(packets.chip_control(0xDC, profile))
                    result['restored_selected_profile'] = query(0xCA)[4]
                    result['saved_profile_unchanged'] = query(0xCC) == original
            finally:
                if fd is not None:
                    os.close(fd)
                output_fd, output_name = tempfile.mkstemp(prefix='legion-owner-result-', suffix='.json')
                with os.fdopen(output_fd, 'w') as stream:
                    json.dump(result, stream, indent=2)
                    stream.write('\n')
                os.chmod(output_name, 0o644)
                print(json.dumps(result, indent=2))
                print(output_name)
