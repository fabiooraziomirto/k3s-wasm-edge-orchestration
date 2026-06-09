(module
  (func $add (param $a i32) (param $b i32) (result i32)
    local.get $a
    local.get $b
    i32.add
  )
  (func $test_function (result i32)
    i32.const 1
    i32.const 2
    call $add
  )
  (export "add" (func $add))
  (export "test_function" (func $test_function))
)