// SPDX-License-Identifier: GPL-2.0-only
/* Exact-model diagnostic, derived from this machine's DSDT and Lenovo's caller.
 * No generic ACPI access: only lighting feature 0x06010000, get 0x11/set 0x12.
 * Loading reads state; an explicit root sysfs write is needed to change it.
 */
#include <linux/acpi.h>
#include <linux/dmi.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/slab.h>

#define OTHER_GUID "DC2A8805-3A8C-41BA-A6F7-092E0089CD3B"
#define LIGHTING_GUID "4DD5BD84-15A9-47E2-AD65-CC61A5C62FD0"
#define LIGHTING_OWNER 0x06010000

static DEFINE_MUTEX(probe_lock);
static bool ready, changed;
static int original_ownership = -1;
static int last_written = -1;
module_param(original_ownership, int, 0444);
MODULE_PARM_DESC(original_ownership, "Ownership read at load; never changes on load");

static int get_owner(void)
{
	__le32 feature = cpu_to_le32(LIGHTING_OWNER);
	struct acpi_buffer in = { sizeof(feature), &feature };
	struct acpi_buffer out = { ACPI_ALLOCATE_BUFFER, NULL };
	union acpi_object *object;
	acpi_status status;
	int value = -EPROTO;

	status = wmi_evaluate_method(OTHER_GUID, 0, 0x11, &in, &out);
	if (ACPI_FAILURE(status))
		return -EIO;
	object = out.pointer;
	if (object && object->type == ACPI_TYPE_INTEGER && object->integer.value <= 1)
		value = object->integer.value;
	kfree(out.pointer);
	return value;
}

static int set_owner(unsigned int value)
{
	__le32 request[2] = { cpu_to_le32(LIGHTING_OWNER), cpu_to_le32(value) };
	struct acpi_buffer in = { sizeof(request), request };
	struct acpi_buffer out = { ACPI_ALLOCATE_BUFFER, NULL };
	union acpi_object *object;
	acpi_status status;
	int result = -EPROTO;

	if (value > 1)
		return -EINVAL;
	status = wmi_evaluate_method(OTHER_GUID, 0, 0x12, &in, &out);
	if (ACPI_FAILURE(status))
		return -EIO;
	object = out.pointer;
	if (object && object->type == ACPI_TYPE_INTEGER && object->integer.value == 1)
		result = 0;
	kfree(out.pointer);
	if (!result && get_owner() != value)
		result = -EIO;
	return result;
}

static int ownership_get(char *buffer, const struct kernel_param *parameter)
{
	int value;
	mutex_lock(&probe_lock);
	value = ready ? get_owner() : -EAGAIN;
	mutex_unlock(&probe_lock);
	return value < 0 ? value : scnprintf(buffer, PAGE_SIZE, "%d\n", value);
}

static int ownership_set(const char *buffer, const struct kernel_param *parameter)
{
	unsigned int value;
	int result = kstrtouint(buffer, 0, &value);
	if (result || value > 1)
		return -EINVAL;
	mutex_lock(&probe_lock);
	if (!ready) {
		result = -EAGAIN;
		goto done;
	}
	/* Mark restoration before invoking firmware, including a partial failure. */
	last_written = value;
	changed = true;
	result = set_owner(value);
done:
	mutex_unlock(&probe_lock);
	return result;
}

static const struct kernel_param_ops ownership_ops = {
	.get = ownership_get,
	.set = ownership_set,
};
module_param_cb(ownership, &ownership_ops, NULL, 0600);
MODULE_PARM_DESC(ownership, "Root-only: read current owner; write 0 or 1 explicitly");

static int __init lighting_probe_init(void)
{
	struct acpi_buffer out = { ACPI_ALLOCATE_BUFFER, NULL };
	union acpi_object *object;
	acpi_status status;
	bool one_zone = false;

	if (!dmi_match(DMI_SYS_VENDOR, "LENOVO") || !dmi_match(DMI_PRODUCT_NAME, "83Q7"))
		return -ENODEV;
	status = wmi_query_block(LIGHTING_GUID, 0, &out);
	if (ACPI_FAILURE(status))
		return -ENODEV;
	object = out.pointer;
	if (object && object->type == ACPI_TYPE_BUFFER && object->buffer.length == 7)
		one_zone = object->buffer.pointer[0] == 0 && object->buffer.pointer[1] == 0 &&
			object->buffer.pointer[2] == 9;
	kfree(out.pointer);
	if (!one_zone)
		return -ENODEV;
	original_ownership = get_owner();
	if (original_ownership < 0)
		return original_ownership;
	ready = true;
	pr_info("legion_lighting_probe: OneZone confirmed, original ownership=%d; no settings written\n",
		original_ownership);
	return 0;
}

static void __exit lighting_probe_exit(void)
{
	int owner_now, result;
	mutex_lock(&probe_lock);
	ready = false;
	if (changed) {
		owner_now = get_owner();
		if (owner_now == last_written) {
			result = set_owner(original_ownership);
			if (result)
				pr_err("legion_lighting_probe: ownership restore failed: %d\n", result);
			else
				pr_info("legion_lighting_probe: restored ownership=%d\n", original_ownership);
		} else {
			pr_info("legion_lighting_probe: ownership changed externally or unreadable; left alone\n");
		}
	}
	mutex_unlock(&probe_lock);
}
module_init(lighting_probe_init);
module_exit(lighting_probe_exit);
MODULE_LICENSE("GPL");
MODULE_DESCRIPTION("Legion 83Q7 OneZone lighting ownership diagnostic");
