/*
 * Copyright (c) 1998, 2017, Oracle and/or its affiliates. All rights reserved.
 * DO NOT ALTER OR REMOVE COPYRIGHT NOTICES OR THIS FILE HEADER.
 *
 * This code is free software; you can redistribute it and/or modify it
 * under the terms of the GNU General Public License version 2 only, as
 * published by the Free Software Foundation.
 *
 * This code is distributed in the hope that it will be useful, but WITHOUT
 * ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
 * FITNESS FOR A PARTICULAR PURPOSE.  See the GNU General Public License
 * version 2 for more details (a copy is included in the LICENSE file that
 * accompanied this code).
 *
 * You should have received a copy of the GNU General Public License version
 * 2 along with this work; if not, write to the Free Software Foundation,
 * Inc., 51 Franklin St, Fifth Floor, Boston, MA 02110-1301 USA.
 *
 * Please contact Oracle, 500 Oracle Parkway, Redwood Shores, CA 94065 USA
 * or visit www.oracle.com if you need additional information or have any
 * questions.
 *
 */

#include "precompiled.hpp"
#include "mmtkVMCompanionThread.hpp"
#include "mmtk.h"

MMTkVMCompanionThread::MMTkVMCompanionThread(): NamedThread(),
    _desired_state(_threads_resumed), _reached_state(_threads_resumed),
    _resumption_count(0) {
  set_name("MMTK VM Companion Thread");
}

MMTkVMCompanionThread::~MMTkVMCompanionThread() {
  guarantee(false, "MMTkVMCompanionThread deletion must fix the race with VM termination");
}

void MMTkVMCompanionThread::run() {
  this->initialize_named_thread();

  log_info(gc)("[companion] Welcome. I am MMTkVMCompanionThread. tid=%d, addr=%p", gettid(), this);

  for (;;) {
    // Wait for suspend request
    log_info(gc)("[companion] Waiting for suspend request...");
    {
      MutexLockerEx locker(VMCompanionThread_lock, true);
      assert(_reached_state == _threads_resumed, "Threads should be running at this moment.");
      while (_desired_state != _threads_suspended) {
        VMCompanionThread_lock->wait(true);
      }
      assert(_reached_state == _threads_resumed, "Threads should still be running at this moment.");
    }

    // Let the VM thread stop the world.
    log_info(gc)("[companion] Letting VMThread execute VM op...");
    VM_MMTkSTWOperation op(this);
    VMThread::execute(&op);
    log_info(gc)("[companion] Returned from VMThread::execute...");

    // Tell the waiter thread that the world has resumed.
    log_info(gc)("[companion] Notifying threads resumption...");
    {
      MutexLockerEx locker(VMCompanionThread_lock, true);
      assert(_desired_state == _threads_resumed, "start-the-world should be requested.");
      assert(_reached_state == _threads_suspended, "Threads should still be suspended at this moment.");
      _reached_state = _threads_resumed;
      _resumption_count++;
      VMCompanionThread_lock->notify_all();
    }
    log_info(gc)("[companion] Notified. Proceed to next round.");
  }
}

void MMTkVMCompanionThread::request(stw_state desired_state, bool wait_until_reached) {
  assert(!Thread::current()->is_VM_thread(), "Requests can only be made by GC threads. Found VM thread.");
  assert(Thread::current() != this, "Requests can only be made by GC threads. Found companion thread.");
  assert(!Thread::current()->is_Java_thread(), "Requests can only be made by GC threads. Found Java thread.");

  log_info(gc)("Entered request.");

  MutexLockerEx locker(VMCompanionThread_lock, true);
  log_info(gc)("VMCompanionThread_lock acquired.");
  assert(_desired_state != desired_state, "State %d already requested.", desired_state);
  _desired_state = desired_state;
  log_info(gc)("Desired state is now %d.", _desired_state);
  VMCompanionThread_lock->notify_all();

  if (wait_until_reached) {
    while (_reached_state != desired_state) {
      VMCompanionThread_lock->wait(true);
    }
  }
}

void MMTkVMCompanionThread::wait_for_reached(stw_state desired_state) {
  MutexLockerEx locker(VMCompanionThread_lock, true);
  assert(_desired_state == desired_state, "State %d not requested.", desired_state);

  while (_reached_state != desired_state) {
    VMCompanionThread_lock->wait(true);
  }
}

void MMTkVMCompanionThread::wait_for_next_resumption() {
  MutexLockerEx locker(VMCompanionThread_lock, false);
  size_t my_count = _resumption_count;
  size_t next_count = my_count + 1;

  while (_resumption_count != next_count) {
    VMCompanionThread_lock->wait(false, 0, true);
  }
}

void MMTkVMCompanionThread::reach_suspended_and_wait_for_resume() {
  assert(Thread::current()->is_VM_thread(), "reach_suspended_and_wait_for_resume can only be executed by the VM thread");

  log_info(gc)("[VMOp] Acquiring VMCompanionThread_lock...");
  MutexLockerEx locker(VMCompanionThread_lock, true);

  // Tell the waiter thread that the world has stopped.
  log_info(gc)("[VMOp] Telling waiter the world stopped...");
  _reached_state = _threads_suspended;
  VMCompanionThread_lock->notify_all();

  // Wait until resume-the-world is requested
  log_info(gc)("[VMOp] Waiting for resumption signal...");
  while (_desired_state != _threads_resumed) {
    log_info(gc)("[VMOp]   Waiting for resumption signal. _desired_state is %d", _desired_state);
    VMCompanionThread_lock->wait(true);
  }
  log_info(gc)("[VMOp] Now desired state is resumed.");
}