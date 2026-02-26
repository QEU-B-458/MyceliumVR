(module
  ;; --- HOST IMPORTS (wasvy:ecs/app) ---
  (import "wasvy:ecs/app" "[constructor]system" (func $sys_new (param i32 i32) (result i32)))
  (import "wasvy:ecs/app" "[method]system.add-query" (func $sys_add_query (param i32 i32 i32)))
  (import "wasvy:ecs/app" "[method]app.add-systems" (func $app_add_systems (param i32 i32 i32 i32 i32 i32)))
  (import "wasvy:ecs/app" "[method]query.iter" (func $query_iter (param i32 i32)))
  (import "wasvy:ecs/app" "[method]query-result.component" (func $res_comp (param i32 i32) (result i32)))

  ;; --- HOST IMPORTS (game:components/components) ---
  (import "game:components/components" "[method]health.heal" (func $health_heal (param i32 f32)))
  (import "game:components/components" "[method]health.pct" (func $health_pct (param i32) (result f32)))

  ;; --- MEMORY & DATA ---
  (memory (export "memory") 1)

  ;; String Data for Registration
  (data (i32.const 0) "heal-system")
  (data (i32.const 12) "pct-system")
  (data (i32.const 30) "game:components/components/health")

  ;; --- SETUP (Called once at startup) ---
  (func (export "setup") (param $app_handle i32)
    (local $heal_sys i32)
    (local $pct_sys i32)

    ;; 1. Initialize heal-system
    i32.const 0 i32.const 11 call $sys_new local.set $heal_sys
    ;; Add Query: Mut("game:components/components/health")
    i32.const 500 i32.const 1 i32.store     ;; Tag 1 = Mut
    i32.const 504 i32.const 30 i32.store    ;; Ptr to path
    i32.const 508 i32.const 33 i32.store    ;; Len
    local.get $heal_sys i32.const 500 i32.const 1 call $sys_add_query

    ;; 2. Initialize pct-system
    i32.const 12 i32.const 10 call $sys_new local.set $pct_sys
    ;; Add Query: Ref("game:components/components/health")
    i32.const 520 i32.const 0 i32.store     ;; Tag 0 = Ref
    i32.const 524 i32.const 30 i32.store    ;; Ptr to path
    i32.const 528 i32.const 33 i32.store    ;; Len
    local.get $pct_sys i32.const 520 i32.const 1 call $sys_add_query

    ;; 3. Register both with Bevy App Schedule::Update (Tag 2)
    i32.const 1024 local.get $heal_sys i32.store
    i32.const 1028 local.get $pct_sys i32.store
    local.get $app_handle
    i32.const 2             ;; Schedule Tag
    i32.const 0 i32.const 0 ;; No custom string name
    i32.const 1024          ;; List pointer
    i32.const 2             ;; List length
    call $app_add_systems
  )

  ;; --- HEAL SYSTEM (Runs every frame) ---
  (func (export "heal-system") (param $query_handle i32)
    (local $res_handle i32)
    (local $health_handle i32)

    block $loop_break
      loop $while_iter
        ;; Get next entity. Option result stored at 2048
        local.get $query_handle i32.const 2048 call $query_iter
        
        ;; If Tag is 0 (None), loop is done
        i32.const 2048 i32.load i32.eqz br_if $loop_break

        ;; Tag 1 (Some), load result handle from 2052
        i32.const 2052 i32.load local.set $res_handle
        
        ;; Get component at index 0 (Health)
        local.get $res_handle i32.const 0 call $res_comp local.set $health_handle

        ;; Call host logic: heal(0.1)
        local.get $health_handle f32.const 0.1 call $health_heal
        
        br $while_iter
      end
    end
  )

  ;; --- PCT SYSTEM (Runs every frame) ---
  (func (export "pct-system") (param $query_handle i32)
    (local $res_handle i32)
    (local $health_handle i32)

    block $loop_break
      loop $while_iter
        local.get $query_handle i32.const 2048 call $query_iter
        i32.const 2048 i32.load i32.eqz br_if $loop_break

        i32.const 2052 i32.load local.set $res_handle
        local.get $res_handle i32.const 0 call $res_comp local.set $health_handle

        ;; Get percentage (result is f32 on stack)
        local.get $health_handle call $health_pct
        drop ;; We just want to ensure it's callable; host side logs the actual values
        
        br $while_iter
      end
    end
  )
)