der_single = polyder([3, 2, 1]);
der_const = polyder(5);
der_prod = polyder([1, 2], [1, 0, -1]);
[quot_num, quot_den] = polyder([1, 0, -1], [1, 1]);
complex_der = polyder(complex([1, -3, 2], [0, -1, 2]));
[complex_quot_num, complex_quot_den] = polyder(complex([1 0], [0 1]), complex([1 0], [0, -1]));

int_basic = polyint([6, 2]);
int_const = polyint(4, 3);
int_col = polyint([2; -1; 4]);
complex_int = polyint(complex([2, 0, 4], [0, -1, 0]));
complex_int_const = polyint(1i, 3);
