"""Set one uniform keyboard colour on the confirmed Lenovo 83Q7 OneZone model.

Uses the visually verified static-profile sequence. Saves diagnostic backups;
changes stored profile 2, which Fn+Space subsequently replays. No sudo needed
when the desktop user already has read/write access to hidraw0.
"""
import argparse
import fcntl
import json
import os
from pathlib import Path
import tempfile
import time
from packets import report, autonomous, chip_control, static_colour


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('colour', help='six hex digits, e.g. FF0000 or 0000FF')
    parser.add_argument('--brightness', type=int, choices=range(1, 10), default=9)
    args = parser.parse_args()
    try:
        colour = bytes.fromhex(args.colour.removeprefix('#'))
        if len(colour) != 3:
            raise ValueError()
    except ValueError:
        parser.error('colour must contain six hex digits')
    if Path('/sys/class/dmi/id/product_name').read_text().strip() != '83Q7':
        raise SystemExit('Restricted to the confirmed Lenovo product 83Q7.')
    if '0000048D:0000C615' not in Path('/sys/class/hidraw/hidraw0/device/uevent').read_text():
        raise SystemExit('Expected keyboard is not hidraw0; refusing writes.')
    fd = os.open('/dev/hidraw0', os.O_RDWR | os.O_CLOEXEC)
    backup = Path(tempfile.mkdtemp(prefix='legion-onezone-set-'))

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
            raise RuntimeError('Unexpected query response')
        return bytes(buf[:n])

    try:
        for command, name in ((0xCA, 'selected-profile'), (0xCD, 'brightness'), (0xCC, 'stored-profile')):
            data = query(command)
            (backup / (name + '.bin')).write_bytes(data)
            if command == 0xCC and (len(data) != 960 or data[4] != 2):
                raise RuntimeError('Stored profile differs from confirmed profile 2; stopping before colour writes.')
        target = static_colour(2, *colour)
        (backup / 'requested-cb.bin').write_bytes(target)
        print('Backup:', backup, flush=True)
        for packet in (autonomous(True), target, report(0xC8, b'\x02'),
                       report(0xCE, bytes((args.brightness,))), chip_control(0xDC, 2)):
            send(packet)
        observed = query(0xCC)
        (backup / 'readback-cc.bin').write_bytes(observed)
        matches = observed[4:] == target[4:]
        (backup / 'result.json').write_text(json.dumps({'colour': colour.hex(),
            'brightness': args.brightness, 'stored_payload_matches': matches}, indent=2) + '\n')
        print('Stored colour payload matches:', matches)
        print('Check the keyboard; if dark, cycle Fn+Space to the lit setting.')
        if not matches:
            raise SystemExit('Stored readback differs; reports retained for diagnosis.')
    finally:
        os.close(fd)


if __name__ == '__main__':
    main()
