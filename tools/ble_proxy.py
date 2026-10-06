#!/usr/bin/env python3
"""FLOW 8 BLE pass-through logger.

The PC connects to the real FLOW 8 (central) and advertises itself as
"FLOW 8 LE" (peripheral). The phone app connects to the PC; every packet is
relayed both ways and logged, so app-only features (routing, phones level,
footswitch config...) can be mapped. Type a line + Enter while running to drop
a marker into the log ("phones vol up").

Close the desktop app's BLE connection first: the mixer takes one client.
Needs only system python3 (dbus-python + gi). Run as root or bluetooth group.

    python3 tools/ble_proxy.py [--mac F7:1A:..] [--log file] [--show-stream]
    python3 tools/ble_proxy.py --selftest
"""
import argparse
import datetime
import sys
import time

import dbus
import dbus.mainloop.glib
import dbus.service
from gi.repository import GLib

BUS = "org.bluez"
OM = "org.freedesktop.DBus.ObjectManager"
PROPS = "org.freedesktop.DBus.Properties"
GATT_SVC = "org.bluez.GattService1"
GATT_CHRC = "org.bluez.GattCharacteristic1"
ADV = "org.bluez.LEAdvertisement1"

SVC_UUID = "14839ad4-8d7e-415c-9a42-167340cf2339"
CHR_UUID = "0034594a-a8e7-4b1a-a6b1-cd5243059a57"
MIXER_NAME = "FLOW 8 LE"

# From docs/flow8-midi-implementation.md §3.4
TYPES = {
    0x06: "ParamChange", 0x07: "ConfigReq", 0x21: "Subscribe", 0x22: "StateStream",
    0x25: "ParamResp", 0x26: "ParamQuery", 0x27: "SnapshotNames", 0x35: "Identity",
    0x36: "AuthAck", 0x37: "SessionStart", 0x38: "StateDump", 0x39: "AuthKey",
    0x4B: "DumpTrigger",
}


def checksum_ok(b):
    return len(b) > 1 and sum(b[:-1]) & 0xFF == b[-1]


def describe(b):
    name = TYPES.get(b[0], f"UNKNOWN_{b[0]:02x}") if b else "empty"
    return f"{name}{'' if checksum_ok(b) else ' BADSUM'}"


class Log:
    def __init__(self, path, show_stream):
        self.f = open(path, "a", buffering=1)
        self.show_stream = show_stream
        self.last = {}  # direction -> last packet, to keep repeats off the console
        print(f"logging to {path}")

    def _out(self, line, console=True):
        line = f"{datetime.datetime.now():%H:%M:%S.%f}"[:-3] + " " + line
        self.f.write(line + "\n")
        if console:
            print(line, flush=True)

    def pkt(self, direction, b):
        # Metering (0x22) and the mixer's pre-auth identity beacon (0x35 every 0.5 s) repeat a lot.
        console = (self.show_stream or not (b and b[0] == 0x22)) and self.last.get(direction) != b
        self.last[direction] = b
        self._out(f"{direction} {b.hex(' ')}  [{describe(b)}]", console)

    def note(self, text):
        self._out(f"### {text}")


def objects(bus):
    return dbus.Interface(bus.get_object(BUS, "/"), OM).GetManagedObjects()


def find_adapter(bus):
    for path, ifs in objects(bus).items():
        if "org.bluez.GattManager1" in ifs and "org.bluez.LEAdvertisingManager1" in ifs:
            return path
    sys.exit("no BLE adapter with GATT server + advertising support")


def find_mixer(bus, adapter_path, mac, timeout=30):
    def match():
        for path, ifs in objects(bus).items():
            d = ifs.get("org.bluez.Device1")
            if not d or not path.startswith(adapter_path + "/"):
                continue
            if (mac and str(d.get("Address", "")).lower() == mac.lower()) or \
               (not mac and MIXER_NAME in (d.get("Name"), d.get("Alias"))):
                return path, d
        return None

    adapter = dbus.Interface(bus.get_object(BUS, adapter_path), "org.bluez.Adapter1")
    adapter.SetDiscoveryFilter({"Transport": "le"})
    adapter.StartDiscovery()
    print("scanning for the mixer...")
    try:
        deadline = time.time() + timeout
        while time.time() < deadline:
            # Want RSSI too, so a cached-but-absent device isn't picked blindly.
            hit = match()
            if hit and ("RSSI" in hit[1] or hit[1].get("Connected")):
                return hit
            time.sleep(0.5)
        hit = match()
        if hit:
            return hit
    finally:
        try:
            adapter.StopDiscovery()
        except dbus.DBusException:
            pass
    sys.exit("FLOW 8 not found — powered on, in range, and not connected to another client?")


class Exported(dbus.service.Object):
    """Minimal BlueZ object: subclasses fill IFACE and props()."""
    IFACE = None

    @dbus.service.method(PROPS, in_signature="s", out_signature="a{sv}")
    def GetAll(self, iface):
        return self.props()[iface]

    @dbus.service.method(PROPS, in_signature="ss", out_signature="v")
    def Get(self, iface, name):
        return self.props()[iface][name]

    @dbus.service.signal(PROPS, signature="sa{sv}as")
    def PropertiesChanged(self, iface, changed, invalidated):
        pass


class Service(Exported):
    def __init__(self, bus, path):
        self.path = path
        super().__init__(bus, path)

    def props(self):
        return {GATT_SVC: {"UUID": SVC_UUID, "Primary": True}}


class Characteristic(Exported):
    def __init__(self, bus, path, svc_path, flags, proxy):
        self.path, self.svc_path, self.flags, self.proxy = path, svc_path, flags, proxy
        self.value, self.notifying = b"", False
        super().__init__(bus, path)

    def props(self):
        return {GATT_CHRC: {
            "Service": dbus.ObjectPath(self.svc_path), "UUID": CHR_UUID,
            "Flags": dbus.Array(self.flags, "s"),
            "Value": dbus.Array(self.value, "y"),
        }}

    @dbus.service.method(GATT_CHRC, in_signature="a{sv}", out_signature="ay")
    def ReadValue(self, options):
        return dbus.Array(self.value, "y")

    @dbus.service.method(GATT_CHRC, in_signature="aya{sv}")
    def WriteValue(self, value, options):
        self.proxy.from_phone(bytes(value), str(options.get("type", "request")))

    @dbus.service.method(GATT_CHRC)
    def StartNotify(self):
        if not self.notifying:
            self.notifying = True
            self.proxy.phone_subscribed()

    @dbus.service.method(GATT_CHRC)
    def StopNotify(self):
        self.notifying = False
        self.proxy.log.note("phone unsubscribed")

    def notify(self, data):
        self.value = data
        if self.notifying:
            self.PropertiesChanged(GATT_CHRC, {"Value": dbus.Array(data, "y")}, [])


class Application(dbus.service.Object):
    def __init__(self, bus, children):
        self.path, self.children = "/flow8proxy", children
        super().__init__(bus, self.path)

    @dbus.service.method(OM, out_signature="a{oa{sa{sv}}}")
    def GetManagedObjects(self):
        return {dbus.ObjectPath(c.path): c.props() for c in self.children}


class Advertisement(Exported):
    """Same fields as the real mixer (128-bit UUID, 32-bit service data, name),
    kept within legacy 31-byte PDUs so phones that only scan legacy see it."""

    def __init__(self, bus, service_data):
        self.path = "/flow8proxy/adv0"
        self.service_data = service_data
        super().__init__(bus, self.path)

    def props(self):
        # BlueZ won't put a name in the scan response, so the split is swapped:
        # name + service data in ADV, 128-bit UUID in SCAN_RSP. Android merges both.
        p = {"Type": "peripheral", "LocalName": MIXER_NAME,
             "ScanResponseServiceUUIDs": dbus.Array([SVC_UUID], "s")}
        if self.service_data:
            # Key as "14839ad4", not the full Base-UUID form: BlueZ would encode it as
            # 128-bit, overflow 31 bytes and fall back to extended adverts phones don't scan.
            p["ServiceData"] = dbus.Dictionary(
                {str(k)[:8] if str(k).endswith("-0000-1000-8000-00805f9b34fb") else k: v
                 for k, v in self.service_data.items()}, "sv")
        return {ADV: p}

    @dbus.service.method(ADV)
    def Release(self):
        pass


class Proxy:
    def __init__(self, bus, log, mixer_chr_path, flags):
        self.bus, self.log = bus, log
        self.mixer = dbus.Interface(bus.get_object(BUS, mixer_chr_path), GATT_CHRC)
        self.identity = []  # 0x35 packets the mixer sent us at connect; replayed to the phone
        svc = Service(bus, "/flow8proxy/service0")
        self.chr = Characteristic(bus, "/flow8proxy/service0/char0", svc.path, flags, self)
        self.app = Application(bus, [svc, self.chr])
        bus.add_signal_receiver(self.from_mixer, "PropertiesChanged", PROPS, BUS,
                                mixer_chr_path)

    def from_mixer(self, iface, changed, invalidated):
        if iface != GATT_CHRC or "Value" not in changed:
            return
        data = bytes(changed["Value"])
        self.log.pkt("MIX>PH", data)
        if data and data[0] == 0x35 and data not in self.identity:
            self.identity.append(data)
        self.chr.notify(data)

    def from_phone(self, data, write_type):
        self.log.pkt("PH>MIX", data)
        self.mixer.WriteValue(dbus.Array(data, "y"), {"type": write_type},
                              reply_handler=lambda: None,
                              error_handler=lambda e: self.log.note(f"mixer write failed: {e}"))

    def phone_subscribed(self):
        self.log.note(f"phone subscribed; replaying {len(self.identity)} identity packet(s)")
        for pkt in self.identity:
            self.chr.notify(pkt)


def connect_mixer(bus, dev_path, log):
    dev = dbus.Interface(bus.get_object(BUS, dev_path), "org.bluez.Device1")
    dev_props = dbus.Interface(bus.get_object(BUS, dev_path), PROPS)
    print(f"connecting to {dev_path} ...")
    try:
        dev.Connect(timeout=40)
    except dbus.DBusException as e:
        if "AlreadyConnected" not in e.get_dbus_name():
            raise
    for _ in range(60):
        if dev_props.Get("org.bluez.Device1", "ServicesResolved"):
            break
        time.sleep(0.5)
    for path, ifs in objects(bus).items():
        c = ifs.get(GATT_CHRC)
        if c and path.startswith(dev_path + "/") and str(c["UUID"]).lower() == CHR_UUID:
            flags = [str(f) for f in c["Flags"]]
            log.note(f"mixer characteristic {path} flags={flags}")
            return dev, path, flags
    dev.Disconnect()
    sys.exit("connected, but FLOW 8 characteristic not found (firmware differs?)")


def selftest():
    # Packets captured in docs/flow8-midi-implementation.md §3.3
    for h in ["370138", "2601b0d7", "260180a7", "4b014c", "360137",
              "3901fd062b0639f17fe7b7278b8f355a495c2a"]:
        assert checksum_ok(bytes.fromhex(h)), h
    assert not checksum_ok(bytes.fromhex("370139"))
    assert describe(bytes.fromhex("360137")) == "AuthAck"
    assert describe(bytes.fromhex("990199")).startswith("UNKNOWN_99 BADSUM")
    print("selftest ok")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--mac", help="mixer MAC (default: find by name)")
    ap.add_argument("--log", default=f"flow8-capture-{datetime.datetime.now():%Y%m%d-%H%M%S}.log")
    ap.add_argument("--show-stream", action="store_true", help="print 0x22 metering on console too")
    ap.add_argument("--selftest", action="store_true")
    args = ap.parse_args()
    if args.selftest:
        return selftest()

    dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
    bus = dbus.SystemBus()
    log = Log(args.log, args.show_stream)
    adapter_path = find_adapter(bus)
    dev_path, dev = find_mixer(bus, adapter_path, args.mac)
    log.note(f"mixer {dev.get('Address')} adv: UUIDs={list(dev.get('UUIDs', []))} "
             f"ManufacturerData={ {int(k): bytes(v).hex() for k, v in dev.get('ManufacturerData', {}).items()} } "
             f"ServiceData={ {str(k): bytes(v).hex() for k, v in dev.get('ServiceData', {}).items()} }")
    mixer_dev, chr_path, flags = connect_mixer(bus, dev_path, log)
    if "notify" not in flags and "indicate" not in flags:
        flags.append("notify")

    proxy = Proxy(bus, log, chr_path, flags)
    mixer_chr = dbus.Interface(bus.get_object(BUS, chr_path), GATT_CHRC)
    mixer_chr.StartNotify()

    adapter_props = dbus.Interface(bus.get_object(BUS, adapter_path), PROPS)
    old_alias = adapter_props.Get("org.bluez.Adapter1", "Alias")
    adapter_props.Set("org.bluez.Adapter1", "Alias", MIXER_NAME)  # GAP Device Name the phone reads

    loop = GLib.MainLoop()
    gatt_mgr = dbus.Interface(bus.get_object(BUS, adapter_path), "org.bluez.GattManager1")
    adv_mgr = dbus.Interface(bus.get_object(BUS, adapter_path), "org.bluez.LEAdvertisingManager1")
    fail = lambda what: lambda e: (log.note(f"{what} failed: {e}"), loop.quit())
    gatt_mgr.RegisterApplication(proxy.app.path, {},
                                 reply_handler=lambda: log.note("GATT server up"),
                                 error_handler=fail("RegisterApplication"))

    adv = Advertisement(bus, dev.get("ServiceData"))

    def adv_err(e):
        # If this BlueZ insists on a Flags field the mirror overflows 31 bytes; drop service data.
        if adv.service_data:
            log.note(f"advertising with service data failed ({e}); retrying without")
            adv.service_data = None
            adv_mgr.RegisterAdvertisement(adv.path, {}, reply_handler=adv_ok,
                                          error_handler=fail("RegisterAdvertisement"))
        else:
            fail("RegisterAdvertisement")(e)

    def adv_ok():
        log.note(f"advertising as '{MIXER_NAME}' — connect the phone app now. "
                 "Type a note + Enter to mark actions; Ctrl-C to stop.")

    adv_mgr.RegisterAdvertisement(adv.path, {}, reply_handler=adv_ok, error_handler=adv_err)

    def on_stdin(fd, cond):
        line = sys.stdin.readline()
        if not line:
            return False
        if line.strip():
            log.note(line.strip())
        return True

    if sys.stdin.isatty():
        GLib.io_add_watch(sys.stdin, GLib.IO_IN, on_stdin)

    def on_mixer_props(iface, changed, invalidated):
        if iface == "org.bluez.Device1" and changed.get("Connected") is False:
            log.note("mixer disconnected — stopping")
            loop.quit()
    bus.add_signal_receiver(on_mixer_props, "PropertiesChanged", PROPS, BUS, dev_path)

    try:
        loop.run()
    except KeyboardInterrupt:
        pass
    finally:
        for step in (lambda: adv_mgr.UnregisterAdvertisement(adv.path),
                     lambda: gatt_mgr.UnregisterApplication(proxy.app.path),
                     lambda: adapter_props.Set("org.bluez.Adapter1", "Alias", old_alias),
                     lambda: mixer_chr.StopNotify(),
                     lambda: mixer_dev.Disconnect()):
            try:
                step()
            except dbus.DBusException:
                pass
        log.note("stopped")


if __name__ == "__main__":
    main()
