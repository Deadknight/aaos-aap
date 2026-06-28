# ARDA AAOS AOA USB Gadget

This note documents the AAOS/Raspberry Pi 4 USB-C gadget path used by ARDA.

The goal is to make the Pi USB-C port expose the Android Open Accessory function required by the AAP/Android Auto daemon:

```text
dwc2 peripheral mode -> configfs usb_gadget -> accessory.usb0 -> /dev/usb_accessory
```

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

The Rust daemon auto-checks this gadget before AAP listener/session commands and creates it only when it is missing. Normal use does not require a separate gadget command.

Auto preflight runs before these commands:

```text
aap-tcp-probe
aap-frame-probe
aap-version-session
aap-ssl-frame-probe
aap-tls-handshake-probe
aap-tls-plain-probe
aap-service-discovery-probe
aap-session-probe
```

Disable the preflight for debugging with:

```sh
ARDA_AAOS_AOA_AUTO=0 arda-senderd aap-session-probe ...
```

The manual command remains available for diagnostics or one-off setup:

```sh
arda-senderd aaos-aoa-gadget --check
arda-senderd aaos-aoa-gadget --unbind
arda-senderd aaos-aoa-gadget --udc fe980000.usb
```

Default gadget values:

```text
gadget name: arda
idVendor:    0x18D1
idProduct:   0x2D00
serial:      arda-aaos
manufacturer: aa-proxy
product:     AAOS AOA
function:    accessory.usb0
```

The auto preflight/manual command does the same setup that was validated manually:

```text
mount configfs if needed
create /config/usb_gadget/arda
create functions/accessory.usb0
link it into configs/c.1 using an absolute configfs symlink
bind to fe980000.usb
ensure /dev/usb_accessory exists when /proc/misc exposes usb_accessory
```

The absolute symlink is intentional. On this image, relative symlinks such as this failed:

```sh
ln -s functions/accessory.usb0 configs/c.1/accessory.usb0
```

The working form is:

```sh
ln -s /config/usb_gadget/arda/functions/accessory.usb0 /config/usb_gadget/arda/configs/c.1/accessory.usb0
```

## Boot/service integration

If ARDA already has an init service that starts `arda-senderd aap-session-probe ...`, no separate gadget service is required; the daemon preflight will run first. The included `arda-aoa-gadget.rc` remains useful as an optional boot preflight for images where you want `/dev/usb_accessory` ready even before starting the main AAP daemon.

Install the binary as:

```text
/vendor/bin/arda-senderd
```

Install this init rc file as:

```text
/vendor/etc/init/arda-aoa-gadget.rc
```

Content:

```rc
service arda_aoa_gadget /vendor/bin/arda-senderd aaos-aoa-gadget --udc fe980000.usb
    class late_start
    user root
    group root usb shell system
    oneshot
    seclabel u:r:su:s0

on late-init
    start arda_aoa_gadget
```

On this userdebug AAOS image the `u:r:su:s0` label works. On a different build without `su`, the init service may need a proper device-specific SELinux domain or the `seclabel` line removed/changed.

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
