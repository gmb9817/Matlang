q_scalar = quantile([1 3 10 20], 0.25);
q_even = quantile([1 3 10 20], 3);

q_cols = quantile([1 2; 3 5; 5 6], [0.25 0.5 0.75]);
q_rows = quantile([1 2; 3 5; 5 6], [0.25 0.5 0.75], 2);
q_row_dim1 = quantile([1 2 3], [0.25 0.5 0.75], 1);
q_col_dim2 = quantile([1; 2; 3], [0.25 0.5 0.75], 2);
q_dim3_scalar = quantile([1 2; 3 5; 5 6], 0.5, 3);
q_dim3_multi = quantile([1 2; 3 5; 5 6], [0.25 0.5 0.75], 3);

p_cols = prctile([1 2; 3 5; 5 6], [25 50 75]);
p_rows = prctile([1 2; 3 5; 5 6], [25 50 75], 2);
p_row_dim1 = prctile([1 2 3], [25 50 75], 1);
p_col_dim2 = prctile([1; 2; 3], [25 50 75], 2);
p_dim3_scalar = prctile([1 2; 3 5; 5 6], 50, 3);
p_dim3_multi = prctile([1 2; 3 5; 5 6], [25 50 75], 3);
