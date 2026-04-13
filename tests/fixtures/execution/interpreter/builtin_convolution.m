conv_full = conv([1, 2, 3], [1, 1, 1]);
conv_same = conv([1, 2, 3, 4], [1, 1, 1], "same");
conv_valid = conv([1, 2, 3, 4], [1, 1, 1], "valid");
column_conv = conv([1; 2; 3], [1; 1]);

conv2_full = conv2([1, 2; 3, 4], [1, 0; 0, -1]);
conv2_same = conv2([1, 2; 3, 4], [1, 1, 1], "same");
conv2_valid = conv2([1, 2, 3; 4, 5, 6], [1, 0; 0, -1], "valid");
conv2_separable = conv2([1; 0; -1], [1, 2, 1], [1, 2; 3, 4]);
conv2_separable_same = conv2([1; 0; -1], [1, 2, 1], [1, 2; 3, 4], "same");

conv_complex = conv(complex([1 2], [1 0]), complex([1 0], [0, -1]));
conv2_complex = conv2([1, 2; 3, 4], complex([1 0], [0 1]));
[deconv_complex_q, deconv_complex_r] = deconv(complex([1 3 0], [1, -1, -2]), complex([1 0], [0, -1]));
