char_poly = poly([0, -1; 1, 0]);
cayley_zero = polyvalm(char_poly, [0, -1; 1, 0]);
upper_eval = polyvalm([1 2 3], [1, 2; 0, 1]);
const_eval = polyvalm(5, [1 2; 3 4]);
complex_matrix_eval = polyvalm([1 0 1], complex([0 0; 0 0], [0, -1; 1, 0]));
