# ARDA AAOS AOA USB Gadget

> **Current v52 behavior:** the current ARDA AAOS image already initializes USB accessory support. Normal AAP commands no longer create, bind, repair, or `su`-reexec a gadget. `aap-run` consumes `/dev/usb_accessory` directly. Use `usb-accessory-status` for a read-only check. The configfs bootstrap commands in this document are retained for older/foreign images only.

This note documents the AAOS/Raspberry Pi 4 USB-C gadget path used by ARDA.

The goal is to document the Pi's **device-side/HU-facing** Android Open Accessory path used by ARDA:

```text
car or DHU (USB host)
        ↓
Pi USB-C peripheral/gadget
        ↓
platform accessory function
        ↓
/dev/usb_accessory
        ↓
arda-senderd AAP endpoint
```

The `nusb`/AOA host code seen in `aa-proxy-rs` serves the opposite role: the proxy acting as USB host toward a wired Android phone. ARDA itself is the phone-like AAP endpoint, so the normal ARDA path opens `/dev/usb_accessory` instead.

## Verified image

Verified on the Snapp/ARPi based Raspberry Pi 4 AAOS image with kernel:

```text
Linux 5.4.83-v8+
```

Verified runtime state:

```text
/sys/class/udc/fe980000.usb exists
/config/usb_gadget is mounted
functions/accessory.usb0 can be created
/proc/misc contains usb_accessory
/dev/usb_accessory exists
macOS shows Accessory connection
AAOS USB debugging still works on the tested setup
```

## Boot partition requirements

The boot partition must contain the `dwc2` overlay:

```text
overlays/dwc2.dtbo
```

`config.txt` must include:

```ini
otg_mode=0
dtoverlay=dwc2,dr_mode=peripheral
```

`cmdline.txt` must include this boot argument on the single existing cmdline row:

```text
androidboot.usbcontroller=fe980000.usb
```

Do not blindly replace the Pi firmware, kernel, or board DTB on an existing AAOS image:

```text
Image.gz
bcm2711-rpi-4-b.dtb
start4.elf
fixup4.dat
```

Only add `dwc2.dtbo` and append the missing boot/config arguments first.

## Runtime behavior

On the current image, USB gadget ownership belongs to the platform/init configuration. ARDA does not auto-bootstrap it.

Read-only status:

```sh
arda-senderd usb-accessory-status
```

The current AAP baseline opens the accessory character device directly:

```text
/dev/usb_accessory
```

The transport is selected in `config.toml`:

```toml
[transport]
mode = "usb-accessory"
usb_path = "/dev/usb_accessory"
```

Normal start:

```sh
arda-senderd aap-run
```

The legacy gadget mutation command is still available for older images or manual recovery, but it is never invoked automatically:

```sh
arda-senderd aaos-aoa-gadget --check
arda-senderd aaos-aoa-gadget --unbind
arda-senderd aaos-aoa-gadget --udc fe980000.usb
```

## Boot/service integration

For the current AAOS image, do **not** install a separate ARDA gadget init service. The platform USB stack already handles accessory negotiation and exposes `/dev/usb_accessory`.

Normal ARDA startup is therefore:

```text
platform USB/init owns gadget state
        ↓
arda-senderd usb-accessory-status   # read-only diagnostic
        ↓
arda-senderd aap-run                # opens /dev/usb_accessory
```

The old `arda-aoa-gadget.rc`, configfs mutation helper, and install script are retained only for historical/foreign images where the platform does not expose a working accessory function. They are not part of the current baseline.

## Install helpers

Two helper scripts are provided:

```text
scripts/install-aaos-boot-dwc2-macos.sh
scripts/install-aaos-aoa-gadget-adb.sh
```

### 1. Patch the SD boot partition from macOS

After inserting the SD card into macOS:

```sh
scripts/install-aaos-boot-dwc2-macos.sh
```

or pass the mount point explicitly:

```sh
scripts/install-aaos-boot-dwc2-macos.sh /Volumes/boot
```

The script:

```text
backs up config.txt and cmdline.txt
copies packaging/boot/overlays/dwc2.dtbo
adds otg_mode=0 if missing
adds dtoverlay=dwc2,dr_mode=peripheral if missing
adds androidboot.usbcontroller=fe980000.usb to cmdline.txt if missing
keeps cmdline.txt as one line
```

### 2. Install the daemon/init service over ADB

After booting AAOS and ensuring `adb shell` + `su` works:

```sh
scripts/install-aaos-aoa-gadget-adb.sh target/aarch64-linux-android/release/arda-senderd
```

The script:

```text
pushes arda-senderd
remounts /vendor read-write if possible
installs /vendor/bin/arda-senderd
installs /vendor/etc/init/arda-aoa-gadget.rc
runs arda-senderd aaos-aoa-gadget --udc fe980000.usb
prints status
```

## Manual verification

On AAOS root shell:

```sh
ls -la /sys/class/udc
```

Expected:

```text
fe980000.usb
```

```sh
cat /proc/misc | grep -i accessory
```

Expected example:

```text
122 usb_accessory
```

```sh
ls -la /dev/usb_accessory
```

Expected example:

```text
crw-rw---- 1 root usb 10, 122 ... /dev/usb_accessory
```

```sh
cat /config/usb_gadget/arda/UDC
```

Expected:

```text
fe980000.usb
```

```sh
ls -la /config/usb_gadget/arda/configs/c.1
```

Expected:

```text
accessory.usb0 -> /config/usb_gadget/arda/functions/accessory.usb0
```

On macOS:

```sh
system_profiler SPUSBDataType | grep -i -A8 -B4 "18d1\|2d00\|android\|accessory\|google\|aa-proxy"
```

## Troubleshooting

### `/sys/class/udc` is empty

`dwc2` did not initialize.

Check:

```sh
dmesg | grep -i dwc2
```

If no `dwc2 fe980000.usb` line exists, recheck:

```text
boot/overlays/dwc2.dtbo exists
config.txt has otg_mode=0
config.txt has dtoverlay=dwc2,dr_mode=peripheral
cmdline.txt has androidboot.usbcontroller=fe980000.usb
```

### `functions/accessory.usb0` cannot be created

The kernel does not expose the Android accessory configfs function. This image/kernel cannot support AOA this way without a kernel/module change.

### `/dev/usb_accessory` is missing

Check:

```sh
cat /proc/misc | grep -i accessory
```

If a minor number is present, the daemon tries to create the node automatically. Manual fallback:

```sh
mknod /dev/usb_accessory c 10 <minor>
chmod 660 /dev/usb_accessory
chown root:usb /dev/usb_accessory
```

For the verified image:

```sh
mknod /dev/usb_accessory c 10 122
chmod 660 /dev/usb_accessory
chown root:usb /dev/usb_accessory
```

If `/proc/misc` does not contain `usb_accessory`, the required misc device is missing from the kernel.

### `configfs-gadget fe980000.usb: failed to start arda: -22`

Usually the function was not linked into the config or the symlink target was wrong.

Check:

```sh
ls -la /config/usb_gadget/arda/configs/c.1
```

Expected:

```text
accessory.usb0 -> /config/usb_gadget/arda/functions/accessory.usb0
```

Then re-run:

```sh
arda-senderd aaos-aoa-gadget --udc fe980000.usb
```

## Compatibility notes

This should be treated as an ARDA Raspberry Pi 4 AAOS image patch, not a universal AAOS patch.

Likely-compatible target:

```text
Raspberry Pi 4
Snapp/ARPi-style AAOS boot partition with config.txt/cmdline.txt
kernel exposes dwc2, configfs, accessory.usb0, and usb_accessory misc device
/vendor is writable through adb root/su or during image build
```

Other AAOS images may use a different USB controller name, a different partition layout, dynamic/super partitions, AVB/dm-verity, or a kernel without the accessory gadget function.
