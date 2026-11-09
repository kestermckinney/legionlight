# Exact-model lighting ownership probe

This diagnostic is restricted to Lenovo product `83Q7` and a firmware lighting
descriptor identifying OneZone RGB. Loading the module queries ownership only.
It does not attach to or replace the existing Lenovo WMI driver.

The caller and firmware agree on feature `0x06010000`, getter method `0x11`,
setter `0x12`, and valid setter values 0 and 1. The module exposes only that
feature. It restores the original value on unload after an explicit change,
unless another caller has changed it in the meantime. Firmware failures are
reported; a successful build is not proof that this controls the physical lights.

Build against the running kernel:

```bash
make -C diagnostics/legion_lighting_probe
```

Read-only query, using the machine's existing enrolled akmods signing key:

```bash
sudo python3 diagnostics/legion_lighting_probe/test-owner.py
```

Bounded experiment: if ownership is 0, request 1, send Lenovo's recovered one-zone
live-red sequence (AA1 → D0 04/profile 2 → A1 tag 1 red → empty A8), ask
for a visual result, then restore ownership and unload:

```bash
sudo python3 diagnostics/legion_lighting_probe/test-owner.py --test
```

The signing key is used by the kernel's signing tool and is never printed or
copied. The test does not save a CB profile, enroll keys, disable Secure Boot,
install a service, or arrange automatic module loading. The script refuses to
disturb a probe already loaded by another process. Run only one copy at a time.

For the current palette investigation, leave a lit valid OneZone profile active
and run both live paths with ownership enabled:

```bash
sudo python3 diagnostics/legion_lighting_probe/test-owner.py --test --lamp
```

This version reads the selected profile instead of assuming profile 2, records its
current profile and key bytes before changing ownership, and asks for two observations: vendor A1
red, then standard LampArray red. If the current selection is 0, it prompts for Apply profile before making an
ownership change. It records the selection again after changing ownership.
It restores ownership before reselecting the original profile and brightness,
restores both interfaces, checks the saved
CC profile is unchanged, then unloads the probe to restore ownership. It does
not save a CB profile. Compared with the early tests, the selected profile now
has a corrected, valid OneZone layout rather than the old malformed 28-key list.
