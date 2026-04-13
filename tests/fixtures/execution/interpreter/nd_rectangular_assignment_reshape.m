a = zeros(2, 2, 3);
a(1, :, 2:3) = [5 6 7 8];
a_row = a(1, :, 2:3);

b = zeros(2, 2, 3);
b(1, :, 2:3) = [5; 6; 7; 8];
b_col = b(1, :, 2:3);

c = zeros(2, 2, 3);
c(1, :, 2:3) = cat(3, [5 6], [7 8]);
c_exact = c(1, :, 2:3);

cells = cat(3, {0 0; 0 0}, {0 0; 0 0}, {0 0; 0 0});
cells(1, :, 2:3) = {50 60 70 80};
cell_row = cells(1, :, 2:3);

cell_scalar = cat(3, {0 0; 0 0}, {0 0; 0 0}, {0 0; 0 0});
cell_scalar(1, :, 2:3) = {9};
cell_scalar_out = cell_scalar(1, :, 2:3);

clear a b c cells cell_scalar
