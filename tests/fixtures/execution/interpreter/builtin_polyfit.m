linear_fit = polyfit([0 1 2], [1 3 5], 1);
[linear_fit_s, linear_s] = polyfit([0 1 2], [1 3 5], 1);
linear_df = linear_s.df;
linear_normr = linear_s.normr;
linear_rsquared = linear_s.rsquared;

quadratic_fit = polyfit([0; 1; 2; 3], [2; 1; 2; 5], 2);
least_squares_fit = polyfit([0 1 2], [1 2 2], 1);
complex_fit = polyfit([0 1 2], complex([1 3 5], [1, 0, -1]), 1);
[complex_fit_s, complex_s] = polyfit([0 1 2], complex([1 3 5], [1, 0, -1]), 1);
complex_with_s_eval = polyval(complex_fit_s, [0 1 2], complex_s);

[scaled_fit, scaled_s, mu] = polyfit([10 20 30], [4 7 10], 1);
scaled_r = scaled_s.R;
scaled_df = scaled_s.df;
scaled_normr = scaled_s.normr;
scaled_rsquared = scaled_s.rsquared;
scaled_eval = polyval(scaled_fit, [10 15 20 25 30], [], mu);
with_s_eval = polyval(linear_fit_s, [0 1 2], linear_s);
