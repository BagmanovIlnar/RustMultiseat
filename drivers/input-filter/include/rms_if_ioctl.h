/* Copyright (c) RustMultiseat contributors.
 * Shared IOCTL ABI for drivers/input-filter ↔ user-mode.
 * Keep in sync with crates/platform-windows/src/input/ioctl.rs
 */
#pragma once

#ifdef _MSC_VER
#include <winioctl.h>
#else
/* Non-WDK hosts: provide CTL_CODE-compatible macros for documentation builds. */
#ifndef CTL_CODE
#define CTL_CODE(DeviceType, Function, Method, Access) \
  (((DeviceType) << 16) | ((Access) << 14) | ((Function) << 2) | (Method))
#endif
#ifndef METHOD_BUFFERED
#define METHOD_BUFFERED 0
#endif
#ifndef FILE_ANY_ACCESS
#define FILE_ANY_ACCESS 0
#endif
#ifndef FILE_READ_DATA
#define FILE_READ_DATA 0x0001
#endif
#ifndef FILE_WRITE_DATA
#define FILE_WRITE_DATA 0x0002
#endif
#ifndef FILE_READ_ACCESS
#define FILE_READ_ACCESS FILE_READ_DATA
#endif
#ifndef FILE_WRITE_ACCESS
#define FILE_WRITE_ACCESS FILE_WRITE_DATA
#endif
#endif

#ifndef _MSC_VER
#include <stdint.h>
typedef uint32_t UINT32;
typedef uint64_t UINT64;
#endif

#ifdef __cplusplus
extern "C" {
#endif

/** Vendor device type (user-defined range). Must match Rust `RMS_IF_DEVICE_TYPE`. */
#define RMS_IF_DEVICE_TYPE 0x8A50u

/** ABI major advertised by IOCTL_RMS_IF_GET_VERSION. */
#define RMS_IF_ABI_VERSION 1u

/** Symbolic link / DOS device name (without \\.\ prefix in INF). */
#define RMS_IF_CONTROL_DEVICE_NAME "RustMultiseatInputFilter"

#define IOCTL_RMS_IF_GET_VERSION \
  CTL_CODE(RMS_IF_DEVICE_TYPE, 0x800, METHOD_BUFFERED, FILE_READ_ACCESS)

#define IOCTL_RMS_IF_SET_FAIL_OPEN \
  CTL_CODE(RMS_IF_DEVICE_TYPE, 0x801, METHOD_BUFFERED, FILE_WRITE_ACCESS)

#define IOCTL_RMS_IF_SET_ROUTES \
  CTL_CODE(RMS_IF_DEVICE_TYPE, 0x802, METHOD_BUFFERED, FILE_WRITE_ACCESS)

#define IOCTL_RMS_IF_QUERY_STATS \
  CTL_CODE(RMS_IF_DEVICE_TYPE, 0x803, METHOD_BUFFERED, FILE_READ_ACCESS)

/* Natural alignment (MSVC / Rust #[repr(C)]); do not pack(1). */
typedef struct _RMS_IF_VERSION_INFO {
  UINT32 AbiVersion;     /* RMS_IF_ABI_VERSION */
  UINT32 DriverBuild;    /* driver-defined build stamp */
  UINT32 CapsFlags;      /* bit0: fail-open default, bit1: routes supported */
} RMS_IF_VERSION_INFO;

typedef struct _RMS_IF_FAIL_OPEN_REQ {
  UINT32 Enabled; /* 1 = fail-open (pass-through), 0 = armed */
} RMS_IF_FAIL_OPEN_REQ;

typedef struct _RMS_IF_STATS {
  UINT64 PacketsSeen;
  UINT64 PacketsPassed;
  UINT64 PacketsSuppressed;
  UINT64 QueueOverflows;
  UINT32 FailOpen; /* 1 if currently fail-open */
  UINT32 Reserved;
} RMS_IF_STATS;

/** Max route rows in one SET_ROUTES buffer (METHOD_BUFFERED size check). */
#define RMS_IF_MAX_ROUTES 32u

/** UTF-8 instance id capacity (NUL-padded, not necessarily NUL-terminated if full). */
#define RMS_IF_INSTANCE_ID_LEN 128u

#define RMS_IF_ROUTE_F_KEYBOARD 0x1u
#define RMS_IF_ROUTE_F_POINTER  0x2u
/** When set with a seat route, suppress delivery to the host class stack. */
#define RMS_IF_ROUTE_F_SUPPRESS_HOST 0x4u

#define RMS_IF_TABLE_F_REPLACE_ALL 0x1u

typedef struct _RMS_IF_ROUTE_ENTRY {
  UINT32 SeatId; /* 0 = host/emergency pass-through bucket */
  UINT32 Flags;  /* RMS_IF_ROUTE_F_* */
  char InstanceId[RMS_IF_INSTANCE_ID_LEN];
} RMS_IF_ROUTE_ENTRY;

typedef struct _RMS_IF_ROUTE_TABLE {
  UINT32 AbiVersion; /* must be RMS_IF_ABI_VERSION */
  UINT32 EntryCount; /* 0..RMS_IF_MAX_ROUTES */
  UINT32 Flags;      /* RMS_IF_TABLE_F_* */
  UINT32 Reserved;
  RMS_IF_ROUTE_ENTRY Entries[RMS_IF_MAX_ROUTES];
} RMS_IF_ROUTE_TABLE;

#ifdef __cplusplus
}
#endif
