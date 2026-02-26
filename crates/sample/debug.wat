(func (export "heal-system") (param $query_handle i32)
    (local $res_handle i32)
    (local $health_handle i32)

    block $loop_break
      loop $while_iter
        ;; 1. Get the next entity from the query
        local.get $query_handle i32.const 2048 call $query_iter
        
        ;; 2. If the Option is None (0), stop
        i32.const 2048 i32.load i32.eqz br_if $loop_break

        ;; 3. Load the Result Handle (offset 4 from the Option tag)
        i32.const 2052 i32.load local.set $res_handle
        
        ;; 4. Get the Health component (Index 0)
        local.get $res_handle i32.const 0 call $res_comp 
        local.set $health_handle

        ;; 5. DEBUG: If handle is 0, something is wrong with the host binding
        local.get $health_handle
        i32.eqz
        if
          br $while_iter ;; Skip if we got a null handle
        end

        ;; 6. HEAL! (Try 1.0 instead of 0.1 to see a bigger jump)
        local.get $health_handle f32.const 1.0 call $health_heal
        
        br $while_iter
      end
    end
  )