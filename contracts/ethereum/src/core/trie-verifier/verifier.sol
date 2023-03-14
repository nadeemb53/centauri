pragma solidity ^0.8.0;

contract MPTVerifier {
    struct Item {
        bytes32 key;
        bytes value;
    }

    function verifyProof(
        bytes32 root,
        bytes[] memory proof,
        bytes[] memory keys
    ) public pure returns (bool) {}

    function _quickSort(
        bytes32[] memory arr,
        uint256 left,
        uint256 right
    ) public pure returns (bytes32[] memory sortedArr) {
        if (arr.length == 0 || left >= right) {
            return arr;
        }
        if (left >= right) return arr;
        bytes32 p = arr[(left + right) / 2];
        uint256 i = left;
        uint256 j = right;
        while (i < j) {
            while (arr[i] < p) ++i;
            while (arr[j] > p) --j;
            if (arr[i] > arr[j]) {
                (arr[i], arr[j]) = (arr[j], arr[i]);
            } else ++i;
        }

        if (j > left) _quickSort(arr, left, j - 1);
        return _quickSort(arr, j + 1, right);
    }
}
