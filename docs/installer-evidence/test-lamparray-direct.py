"""Temporary blue LampArray test without changing ACPI ownership; no CB writes."""
import fcntl
import os
from pathlib import Path
import select
import sys
import time
from packets import report, autonomous, chip_control

def main():
    assert Path('/sys/class/dmi/id/product_name').read_text().strip() == '83Q7'
    for i in (0,1):
        assert '0000048D:0000C615' in Path(f'/sys/class/hidraw/hidraw{i}/device/uevent').read_text()
    vendor=os.open('/dev/hidraw0',os.O_RDWR|os.O_CLOEXEC)
    lamp=None
    changed=False
    def send(fd,packet):
        n=len(packet)
        if fcntl.ioctl(fd,(3<<30)|(n<<16)|(ord('H')<<8)|6,bytearray(packet),True)!=n:
            raise RuntimeError('Incomplete feature write')
        time.sleep(.05)
    def query(cmd):
        send(vendor,report(cmd,b'' if cmd==0xcc else b'\0'))
        buf=bytearray(960);buf[0]=7
        n=fcntl.ioctl(vendor,(3<<30)|(960<<16)|(ord('H')<<8)|7,buf,True)
        if n<5 or buf[:2]!=bytes((7,cmd)):raise RuntimeError('Unexpected query')
        return bytes(buf[:n])
    try:
        selected=query(0xca)[4]
        before=query(0xcc)
        brightness=query(0xcd)[4]
        # Selection may be 0; the last stored slot is still available for restoration.
        profile=selected if selected in (1,2,3) else before[4]
        if profile not in (1,2,3) or brightness>9:raise RuntimeError('Unexpected current state')
        lamp=os.open('/dev/hidraw1',os.O_RDWR|os.O_CLOEXEC)
        changed=True
        send(vendor,autonomous(False))
        send(lamp,bytes.fromhex('06 00'))
        send(lamp,bytes.fromhex('05 01 00 00 00 00 00 00 ff 01'))
        print('Direct LampArray BLUE active; no ownership probe loaded.',flush=True)
        print('Waiting for Enter or 90-second timeout to restore.',flush=True)
        if select.select([sys.stdin],[],[],90)[0]:sys.stdin.readline()
    finally:
        try:
            if lamp is not None:
                try:
                    if changed:send(lamp,bytes.fromhex('06 01'))
                finally:os.close(lamp)
        finally:
            try:
                if changed:
                    send(vendor,autonomous(True))
                    send(vendor,report(0xc8,bytes((profile,))))
                    send(vendor,report(0xce,bytes((brightness,))))
                    send(vendor,chip_control(0xdc,profile))
                    print('Stored profile unchanged:',query(0xcc)==before,flush=True)
                    if selected==0:send(vendor,report(0xce,b'\0'))
                    print('Firmware control restored.',flush=True)
            finally:os.close(vendor)

if __name__=='__main__':main()
