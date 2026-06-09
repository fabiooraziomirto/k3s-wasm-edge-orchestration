/*
 * Simple WASM test module - C source
 * Compile with WASI SDK or Emscripten
 */

int add(int a, int b) {
    return a + b;
}

int multiply(int a, int b) {
    return a * b;
}

int fibonacci(int n) {
    if (n <= 1) return n;
    return fibonacci(n - 1) + fibonacci(n - 2);
}

int factorial(int n) {
    if (n <= 1) return 1;
    return n * factorial(n - 1);
}

int main() {
    // Test functions
    int result1 = add(5, 3);
    int result2 = multiply(4, 7);
    int result3 = fibonacci(10);
    int result4 = factorial(5);
    return 0;
}

