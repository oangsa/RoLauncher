"""Capture a single X11/XWayland window owned by a verified host PID.

No root-window/desktop fallback. XRes supplies the host PID even for Flatpak;
_NET_WM_PID is deliberately not trusted across PID namespaces. KWin's compositor
supplies the window pixmap, so overlapping windows are never photographed.
Unavailable libraries, native Wayland clients and ambiguous windows fail closed.
The caller bounds runtime and output. No screenshots are written to disk.
"""
import ctypes as c
import ctypes.util
import struct
import sys
import zlib


def main(pid):
    x = c.CDLL(ctypes.util.find_library('X11'))
    res = c.CDLL(ctypes.util.find_library('XRes'))
    composite = c.CDLL(ctypes.util.find_library('Xcomposite'))
    pointer, xid = c.c_void_p, c.c_ulong

    def bind(lib, name, result, *args):
        fn = getattr(lib, name)
        fn.restype, fn.argtypes = result, args
        return fn

    class Spec(c.Structure):
        _fields_ = [('client', xid), ('mask', c.c_uint)]

    class Value(c.Structure):
        _fields_ = [('spec', Spec), ('length', c.c_long), ('value', pointer)]

    class Image(c.Structure):
        _fields_ = [('width', c.c_int), ('height', c.c_int), ('xoffset', c.c_int),
                    ('format', c.c_int), ('data', pointer), ('byte_order', c.c_int),
                    ('bitmap_unit', c.c_int), ('bitmap_bit_order', c.c_int),
                    ('bitmap_pad', c.c_int), ('depth', c.c_int), ('bytes_per_line', c.c_int),
                    ('bits_per_pixel', c.c_int), ('red_mask', xid), ('green_mask', xid), ('blue_mask', xid)]

    display = bind(x, 'XOpenDisplay', pointer, c.c_char_p)(None)
    if not display:
        return
    root = bind(x, 'XDefaultRootWindow', xid, pointer)(display)
    atom = bind(x, 'XInternAtom', xid, pointer, c.c_char_p, c.c_int)(display, b'_NET_CLIENT_LIST', 1)
    actual, fmt, count, after, data = xid(), c.c_int(), xid(), xid(), pointer()
    get_property = bind(x, 'XGetWindowProperty', c.c_int, pointer, xid, xid, c.c_long, c.c_long,
                        c.c_int, xid, c.POINTER(xid), c.POINTER(c.c_int), c.POINTER(xid), c.POINTER(xid), c.POINTER(pointer))
    get_property(display, root, atom, 0, 4096, 0, 0, c.byref(actual), c.byref(fmt), c.byref(count), c.byref(after), c.byref(data))
    if not data.value or fmt.value != 32 or after.value:
        return
    windows = list(c.cast(data, c.POINTER(xid))[:count.value])
    bind(x, 'XFree', c.c_int, pointer)(data)
    query = bind(res, 'XResQueryClientIds', c.c_int, pointer, c.c_long, c.POINTER(Spec), c.POINTER(c.c_long), c.POINTER(c.POINTER(Value)))
    get_pid = bind(res, 'XResGetClientPid', c.c_int, c.POINTER(Value))
    destroy = bind(res, 'XResClientIdsDestroy', None, c.c_long, c.POINTER(Value))
    matches = []
    for window in windows:
        spec, n, values = Spec(window, 2), c.c_long(), c.POINTER(Value)()
        query(display, 1, c.byref(spec), c.byref(n), c.byref(values))
        if values:
            if any(get_pid(c.byref(values[i])) == pid for i in range(n.value)):
                matches.append(window)
            destroy(n, values)
    if len(matches) != 1:
        return
    pixmap = bind(composite, 'XCompositeNameWindowPixmap', xid, pointer, xid)(display, matches[0])
    r, px, py, width, height, border, depth = xid(), c.c_int(), c.c_int(), c.c_uint(), c.c_uint(), c.c_uint(), c.c_uint()
    geometry = bind(x, 'XGetGeometry', c.c_int, pointer, xid, c.POINTER(xid), c.POINTER(c.c_int), c.POINTER(c.c_int), c.POINTER(c.c_uint), c.POINTER(c.c_uint), c.POINTER(c.c_uint), c.POINTER(c.c_uint))
    if not geometry(display, pixmap, c.byref(r), c.byref(px), c.byref(py), c.byref(width), c.byref(height), c.byref(border), c.byref(depth)):
        return
    if not (0 < width.value <= 4096 and 0 < height.value <= 4096):
        return
    img = bind(x, 'XGetImage', c.POINTER(Image), pointer, xid, c.c_int, c.c_int, c.c_uint, c.c_uint, xid, c.c_int)(display, pixmap, 0, 0, width, height, xid(-1), 2)
    if not img:
        return
    info = img.contents
    if info.bits_per_pixel != 32 or info.byte_order != 0 or (info.red_mask, info.green_mask, info.blue_mask) != (0xff0000, 0xff00, 0xff):
        return
    raw = c.string_at(info.data, info.bytes_per_line * height.value)
    rows = []
    nonblack = False
    for y in range(height.value):
        row = raw[y * info.bytes_per_line:y * info.bytes_per_line + width.value * 4]
        rgb = bytearray(width.value * 3)
        rgb[0::3], rgb[1::3], rgb[2::3] = row[2::4], row[1::4], row[0::4]
        nonblack |= any(rgb)
        rows.append(b'\0' + rgb)
    bind(x, 'XDestroyImage', c.c_int, c.POINTER(Image))(img)
    bind(x, 'XFreePixmap', c.c_int, pointer, xid)(display, pixmap)
    bind(x, 'XCloseDisplay', c.c_int, pointer)(display)
    if not nonblack:
        return

    def chunk(kind, value):
        return struct.pack('>I', len(value)) + kind + value + struct.pack('>I', zlib.crc32(kind + value))

    png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width.value, height.value, 8, 2, 0, 0, 0))
    png += chunk(b'IDAT', zlib.compress(b''.join(rows))) + chunk(b'IEND', b'')
    sys.stdout.buffer.write(png)


if __name__ == '__main__':
    try:
        main(int(sys.argv[1]))
    except Exception:
        sys.exit(1)
