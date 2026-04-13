source = cat(3, [1 2; 3 4], [5 6; 7 8]);
matrix_out = zeros(1, 8);
matrix_out(:) = source;

cell_source = cat(3, {1 2; 3 4}, {5 6; 7 8});
cell_out = {0, 0, 0, 0, 0, 0, 0, 0};
cell_out{1:8} = cell_source;

nested_out = {{0, 0}, {0, 0}};
[nested_out{:}{:}] = {31, 32; 33, 34};

clear source cell_source;
