fir_row = filter([1 1], 1, [1 2 3 4]);
[fir_row_2, fir_state] = filter([1 1], 1, [1 2 3 4]);

col_iir = filter(1, [1, -0.5], [1; 0; 0]);
col_zi = filter([1 1], 1, [5; 6], 10);

[mat_default_2, default_state] = filter([1 1], 1, [1 2 3; 4 5 6]);
mat_default = filter([1 1], 1, [1 2 3; 4 5 6]);
[mat_rows, row_state] = filter([1 1], 1, [1 2 3; 4 5 6], [], 2);
[mat_pages, page_state] = filter([1 1], 1, [1 2 3; 4 5 6], [], 3);
[page_default, page_default_state] = filter([1 1], 1, reshape([1 2 3 4], [1 1 4]));
page_default_size = size(page_default);
page_default_ndims = ndims(page_default);
[row_zi, row_zi_state] = filter([1 1], 1, [1 2 3; 4 5 6], [10 20], 2);

[complex_fir, complex_fir_state] = filter(complex([1 0], [0 1]), 1, [1 2 3]);
