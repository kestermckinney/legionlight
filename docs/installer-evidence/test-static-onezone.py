"""Temporarily test the recovered OneZone static profile; restore on Enter/Ctrl+C.

Run as the desktop user. Requires existing read/write access to hidraw0.
Backs up the current profile before writing and checks stored-data restoration.
Does not change ACPI ownership, install drivers, or change the app backend.
"""
import fcntl
import json
import os
from pathlib import Path
import tempfile
import time

from packets import report, autonomous, chip_control


def static_red(profile):
    # OneZone save: a5760, group: a59a0, animation properties: a6220.
    # Static animation 11, no speed/clockwise/direction properties => 0/0/4.
    # Fixed colour => property 5 = 2, one RGB triple, one LE16 LED ID (1).
    group = bytes.fromhex('01 06 01 0b 02 00 03 00 04 04 05 02 06 00 01 ff 00 00 01 01 00')
    return report(0xCB, bytes((profile, 1, 1)) + group)


def main():
    identity = Path('/sys/class/hidraw/hidraw0/device/uevent').read_text()
    if '0000048D:0000C615' not in identity:
        raise SystemExit('Expected 048d:c615 on hidraw0; refusing writes.')
    if Path('/sys/class/dmi/id/product_name').read_text().strip() != '83Q7':
        raise SystemExit('This diagnostic is restricted to product 83Q7.')
    fd = os.open('/dev/hidraw0', os.O_RDWR | os.O_CLOEXEC)
    output = Path(tempfile.mkdtemp(prefix='legion-static-onezone-'))
    result = {}
    changed = False

    def send(packet):
        n = fcntl.ioctl(fd, (3 << 30) | (960 << 16) | (ord('H') << 8) | 6,
                        bytearray(packet), True)
        if n != 960:
            raise RuntimeError('Incomplete feature write')
        time.sleep(.15)

    def query(command):
        send(report(command, b'' if command == 0xCC else b'\0'))
        buf = bytearray(960)
        buf[0] = 7
        n = fcntl.ioctl(fd, (3 << 30) | (960 << 16) | (ord('H') << 8) | 7, buf, True)
        if n < 5 or buf[:2] != bytes((7, command)):
            raise RuntimeError('Unexpected query reply')
        return bytes(buf[:n])

    try:
        current = query(0xCA)
        brightness = query(0xCD)
        original = query(0xCC)
        # Save query evidence even when the preconditions reject the test.
        (output / 'initial-ca.bin').write_bytes(current)
        (output / 'initial-cc.bin').write_bytes(original)
        result['initial_selected_profile'] = current[4]
        result['initial_stored_profile'] = original[4]
        print('Selected profile:', current[4], 'Stored profile:', original[4], flush=True)
        # Fn+Space can leave CA at 0 while CC still returns profile 2.
        # Do not guess what selecting profile 0 does, or overwrite another slot.
        if current[4] != 2:
            print('No profile writes yet. Use Fn+Space to select the lit blue setting.', flush=True)
            for attempt in range(3):
                input('After selecting the lit setting, press Enter to check (Ctrl+C cancels): ')
                current = query(0xCA)
                print('Selected profile:', current[4], flush=True)
                if current[4] == 2:
                    break
            brightness = query(0xCD)
            original = query(0xCC)
        profile = current[4]
        # Limit the test to the previously observed active static profile.
        if profile != 2 or len(original) != 960 or original[4:7] != b'\x02\x01\x01':
            raise RuntimeError('Need selected profile 2 and stored profile 2; no profile writes performed.')
        if brightness[4] > 9:
            raise RuntimeError('Unexpected brightness')
        (output / 'original-cc.bin').write_bytes(original)
        (output / 'original-ca.bin').write_bytes(current)
        (output / 'original-cd.bin').write_bytes(brightness)
        restore = bytearray(original)
        restore[1] = 0xCB
        # The DLL transport a3db0 clamps oversized payload length to 956.
        restore[2:4] = min(int.from_bytes(restore[2:4], 'little'), 956).to_bytes(2, 'little')
        (output / 'restore-cb.bin').write_bytes(restore)
        target = static_red(profile)
        (output / 'test-cb.bin').write_bytes(target)
        print('Backup:', output, flush=True)
        changed = True
        send(autonomous(True))
        send(target)
        send(report(0xC8, bytes((profile,))))
        send(report(0xCE, b'\x09'))
        send(chip_control(0xDC, profile))
        observed = query(0xCC)
        (output / 'test-readback-cc.bin').write_bytes(observed)
        result['test_payload_readback_matches'] = observed[4:] == target[4:]
        print('Stored test payload matches:', result['test_payload_readback_matches'], flush=True)
        result['visible_result'] = input('Check the keyboard, then cycle Fn+Space. What colours appear? Enter to restore: ')
    finally:
        try:
            if changed:
                send(restore)
                send(report(0xC8, bytes((profile,))))
                send(report(0xCE, bytes((brightness[4],))))
                send(chip_control(0xDC, profile))
                send(autonomous(True))
                restored = query(0xCC)
                (output / 'restored-cc.bin').write_bytes(restored)
                # The transport clamps original payload length 960 to 956.
                # Verify against that expected header plus every original data byte.
                expected = bytearray(original)
                expected[2:4] = restore[2:4]
                result['raw_readback_identical'] = restored == original
                result['original_profile_readback_restored'] = restored == bytes(expected)
                print('Original stored profile restored:',
                      result['original_profile_readback_restored'], flush=True)
        finally:
            os.close(fd)
            (output / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
            print(json.dumps(result, indent=2))
            print(output)


if __name__ == '__main__':
    main()
