#!/usr/bin/env bash
# Packaging consistency for drivers/input-filter (no WDK required).
# Checks INF ↔ vcxproj TargetName ↔ InstallPlan artifact paths.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

SYS_NAME="RustMultiseatInputFilter"
VCXPROJ="drivers/input-filter/sys/${SYS_NAME}.vcxproj"
INF_KB="drivers/input-filter/inf/RustMultiseatKbFilter.inf"
INF_MOU="drivers/input-filter/inf/RustMultiseatMouFilter.inf"
IOCTL_H="drivers/input-filter/include/rms_if_ioctl.h"

die() { echo "FAIL: $*" >&2; exit 1; }

[[ -f "$VCXPROJ" ]] || die "missing $VCXPROJ"
[[ -f "$INF_KB" ]] || die "missing $INF_KB"
[[ -f "$INF_MOU" ]] || die "missing $INF_MOU"
[[ -f "$IOCTL_H" ]] || die "missing $IOCTL_H"
[[ -f "drivers/input-filter/sys/driver.c" ]] || die "missing driver.c"
[[ -f "drivers/input-filter/sys/control.c" ]] || die "missing control.c"
[[ -f "drivers/input-filter/sys/class_filter.c" ]] || die "missing class_filter.c"
[[ -f "drivers/input-filter/sys/class_filter.h" ]] || die "missing class_filter.h"
grep -q 'RmsIfOnConnect' drivers/input-filter/sys/class_filter.c \
  || die "class_filter.c must define RmsIfOnConnect"
grep -q 'RmsIfOnInput' drivers/input-filter/sys/class_filter.c \
  || die "class_filter.c must define RmsIfOnInput"
grep -q 'WdfFdoInitSetFilter' drivers/input-filter/sys/driver.c \
  || die "driver.c must call WdfFdoInitSetFilter"
grep -q 'RmsIfCreateControlDevice' drivers/input-filter/sys/driver.c \
  || die "driver.c must create control device in DriverEntry"
grep -q 'RmsIfCreateControlDevice' drivers/input-filter/sys/control.c \
  || die "control.c must define RmsIfCreateControlDevice"
grep -q 'RmsIfCaptureInstanceId' drivers/input-filter/sys/class_filter.c \
  || die "class_filter.c must define RmsIfCaptureInstanceId"
grep -q 'class_filter.c' "$VCXPROJ" || die "vcxproj must compile class_filter.c"

grep -q "<TargetName>${SYS_NAME}</TargetName>" "$VCXPROJ" \
  || die "vcxproj TargetName must be ${SYS_NAME}"

for inf in "$INF_KB" "$INF_MOU"; do
  grep -q "${SYS_NAME}.sys" "$inf" \
    || die "$inf must reference ${SYS_NAME}.sys"
done

grep -q 'RustMultiseatKbFilter' "$INF_KB" || die "kb INF service name"
grep -q 'RustMultiseatMouFilter' "$INF_MOU" || die "mou INF service name"
grep -q 'RMS_IF_CONTROL_DEVICE_NAME "RustMultiseatInputFilter"' "$IOCTL_H" \
  || die "IOCTL header control device name mismatch"

# InstallPlan packaging_artifacts must list INF + vcxproj (via cargo test / quick check).
cargo test -q -p multiseat-core install::tests::input_filter_detail_points_at_inf_and_services --lib

echo "verify_input_filter_package: OK (INF ↔ ${SYS_NAME}.sys ↔ vcxproj ↔ InstallPlan)"
