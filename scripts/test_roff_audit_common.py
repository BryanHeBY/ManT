"""Deterministic resource-planning tests for local roff audit drivers."""

from __future__ import annotations

import unittest
from pathlib import Path
from unittest.mock import patch

import roff_audit_common as common
from roff_audit_common import (
    AUDIT_WORKER_MEMORY_RESERVE,
    default_audit_batch_size,
    resolve_audit_parallelism,
)


class AuditParallelismTests(unittest.TestCase):
    def test_automatic_plan_uses_the_tightest_observed_budget(self):
        plan = resolve_audit_parallelism(
            None,
            hard_limit=16,
            cpu_limit=20,
            memory_available=5 * AUDIT_WORKER_MEMORY_RESERVE,
            file_descriptor_limit=160,
        )
        self.assertEqual(plan.workers, 4)
        self.assertEqual(plan.automaticWorkers, 4)
        self.assertEqual(plan.cpuLimit, 20)
        self.assertEqual(plan.memoryLimit, 5)
        self.assertEqual(plan.fileDescriptorLimit, 4)
        self.assertFalse(plan.explicitOverride)

    def test_large_nonlimiting_budgets_do_not_reduce_cpu_capacity(self):
        plan = resolve_audit_parallelism(
            None,
            hard_limit=16,
            cpu_limit=12,
            memory_available=100 * AUDIT_WORKER_MEMORY_RESERVE,
            file_descriptor_limit=100_000,
        )
        self.assertEqual(plan.workers, 12)
        self.assertEqual(plan.memoryLimit, 100)
        self.assertGreater(plan.fileDescriptorLimit, 12)

    def test_explicit_workers_are_bounded_but_can_override_the_advisory_default(self):
        plan = resolve_audit_parallelism(
            12,
            hard_limit=16,
            cpu_limit=4,
            memory_available=AUDIT_WORKER_MEMORY_RESERVE,
            file_descriptor_limit=1024,
        )
        self.assertEqual(plan.automaticWorkers, 1)
        self.assertEqual(plan.workers, 12)
        self.assertTrue(plan.explicitOverride)
        self.assertEqual(plan.report()["requestedWorkers"], 12)
        with self.assertRaisesRegex(ValueError, "workers 1..16"):
            resolve_audit_parallelism(17, hard_limit=16, cpu_limit=20)

    def test_batch_size_is_bounded_and_tracks_workers(self):
        self.assertEqual(default_audit_batch_size(1), 2)
        self.assertEqual(default_audit_batch_size(12), 24)
        self.assertEqual(default_audit_batch_size(16), 32)
        self.assertEqual(default_audit_batch_size(16, maximum=16), 16)
        with self.assertRaises(ValueError):
            default_audit_batch_size(0)

    def test_linux_cgroup_limits_reduce_host_observations(self):
        values = {
            Path("/sys/fs/cgroup/cpu.max"): "250000 100000",
            Path("/proc/meminfo"): "MemAvailable:       8388608 kB\n",
            Path("/sys/fs/cgroup/memory.max"): str(3 * AUDIT_WORKER_MEMORY_RESERVE),
            Path("/sys/fs/cgroup/memory.current"): str(AUDIT_WORKER_MEMORY_RESERVE),
        }
        with patch.object(common.os, "sched_getaffinity", return_value=set(range(12))), \
             patch.object(common, "_read_text", side_effect=lambda path: values.get(path)):
            self.assertEqual(common.audit_cpu_limit(), 3)
            self.assertEqual(common.audit_memory_available(), 2 * AUDIT_WORKER_MEMORY_RESERVE)


if __name__ == "__main__":
    unittest.main()
