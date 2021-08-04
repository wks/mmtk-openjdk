/*
 * Copyright (c) 1998, 2016, Oracle and/or its affiliates. All rights reserved.
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

#ifndef SHARE_GC_MMTK_VMCOMPANIONTHREAD_HPP
#define SHARE_GC_MMTK_VMCOMPANIONTHREAD_HPP

#include "runtime/perfData.hpp"
#include "runtime/thread.hpp"
#include "runtime/vmOperations.hpp"
#include "mmtkVMOperation.hpp"
#include "runtime/mutex.hpp"

class MMTkVMCompanionThread: public NamedThread {
public:
  enum stw_state {
    _threads_suspended,
    _threads_resumed,
  };
private:
  stw_state _desired_state;
  stw_state _reached_state;
  size_t _resumption_count;

public:
  // Constructor
  MMTkVMCompanionThread();
  ~MMTkVMCompanionThread();

  virtual void run() override;

  void request(stw_state desired_state, bool wait_until_reached);
  void wait_for_reached(stw_state reached_state);
  void wait_for_next_resumption();
  void reach_suspended_and_wait_for_resume();
};

#endif // SHARE_GC_MMTK_VMCOMPANIONTHREAD_HPP
