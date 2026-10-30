# Virtual input (experimental)

Virtual HID can create a device; it does **not** by itself address an arbitrary
Windows session. Stage 0 must compare at least:

1. documented virtual HID + session agent;
2. allowable RDP/remote-session input path;
3. physical filter routing (preferred if proven).

Do not select this package in `install-plan` until Stage 0 evidence exists.
See `drivers/input-filter/` for the primary PoC path.
