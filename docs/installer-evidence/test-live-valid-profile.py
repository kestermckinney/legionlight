"""Bounded OneZone live-colour test on an already valid saved profile.
No CB profile writes. Restores firmware control on Enter/Ctrl+C or after 90s.
"""
import fcntl
import os
from pathlib import Path
import select
import sys
import time
from packets import report, autonomous, chip_control, live_colour

def main():
    assert Path('/sys/class/dmi/id/product_name').read_text().strip() == '83Q7'
    assert '0000048D:0000C615' in Path('/sys/class/hidraw/hidraw0/device/uevent').read_text()
    fd = os.open('/dev/hidraw0', os.O_RDWR | os.O_CLOEXEC)
    changed = False
    def send(packet, delay=.15):
        n=fcntl.ioctl(fd,(3<<30)|(960<<16)|(ord('H')<<8)|6,bytearray(packet),True)
        if n != 960: raise RuntimeError('Incomplete feature report')
        time.sleep(delay)
    def query(cmd):
        send(report(cmd,b'' if cmd==0xcc else b'\0'))
        buf=bytearray(960);buf[0]=7
        n=fcntl.ioctl(fd,(3<<30)|(960<<16)|(ord('H')<<8)|7,buf,True)
        if n<5 or buf[:2]!=bytes((7,cmd)):raise RuntimeError('Unexpected query')
        return bytes(buf[:n])
    try:
        profile=query(0xca)[4]
        original=query(0xcc)
        if profile not in (1,2,3) or len(original)!=960 or original[4]!=profile:
            raise RuntimeError('No valid active profile')
        changed=True
        send(autonomous(True))
        send(chip_control(4,profile))
        send(live_colour(0xa1,[(1,255,0,0)]),.005)
        send(live_colour(0xa8,[]),.005)
        print('Live red test active on profile',profile,flush=True)
        print('Waiting for Enter, Ctrl+C, or 90-second timeout to restore.',flush=True)
        if select.select([sys.stdin],[],[],90)[0]:sys.stdin.readline()
    finally:
        try:
            if changed:
                send(chip_control(0xdc,profile))
                send(autonomous(True))
                print('Saved profile unchanged:',query(0xcc)==original,flush=True)
                print('Firmware control restored.',flush=True)
        finally:os.close(fd)

if __name__=='__main__':main()
