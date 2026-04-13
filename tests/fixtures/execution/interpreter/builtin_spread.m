r_cols = range([1 2; 3 5; 5 6]);
r_rows = range([1 2; 3 5; 5 6], 2);
r_dim3 = range([1 2; 3 5; 5 6], 3);
r_row_dim1 = range([1 2 3], 1);
r_col_dim2 = range([1; 2; 3], 2);

[iqr_cols, q_cols] = iqr([1 2; 3 5; 5 6]);
[iqr_rows, q_rows] = iqr([1 2; 3 5; 5 6], 2);
[iqr_row_dim1, q_row_dim1] = iqr([1 2 3], 1);
[iqr_col_dim2, q_col_dim2] = iqr([1; 2; 3], 2);
iqr_dim3 = iqr([1 2; 3 5; 5 6], 3);
[iqr_dim3_full, q_dim3_full] = iqr([1 2; 3 5; 5 6], 3);
