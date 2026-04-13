row_eval = polyval([3, 2, 1], [5 7 9]);
col_eval = polyval([1, -3, 2], [0; 1; 2; 3]);
mat_eval = polyval([1, 0, -1], [1 2; 3 4]);
const_eval = polyval(5, [1 2; 3 4]);
col_coeff_eval = polyval([2; -1; 4], [2 3]);
complex_eval = polyval(complex([1, -3, 2], [0, -1, 2]), [0 3]);
complex_query_eval = polyval([1 0 1], [1i 2]);
struct_eval = polyval([2, 1], [0 1 2], struct());
scaled_eval = polyval([3, 7], [10 15 20 25 30], [], [20 10]);

[ls_fit, ls_s] = polyfit([0 1 2], [1 2 2], 1);
[delta_eval, delta] = polyval(ls_fit, [0 0.5 1 1.5 2], ls_s);

[scaled_fit, scaled_s, scaled_mu] = polyfit([10 20 30], [4 6 11], 1);
[scaled_delta_eval, scaled_delta] = polyval(scaled_fit, [10 15 20 25 30], scaled_s, scaled_mu);
